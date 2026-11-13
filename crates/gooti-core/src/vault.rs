//! The local vault: one JSON file holding sealed identities and the backup state.
//!
//! ```text
//! password ──Argon2id──▶ password key ──opens──▶ data key ──opens──▶ identities + backup state
//! ```
//!
//! The random data key sits between the password and the data, so "Unlock at login" can keep
//! the data key in the OS keychain, and a password change only re-seals 32 bytes.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::activity::{self, ActivityEntry};
use crate::backup::blob::shared_content_chars;
use crate::backup::document::{SharedDocument, is_wss_url};
use crate::backup::phrase::{BackupKeys, derive_desktop_keys, derive_shared_keys};
use crate::backup::state::{
    BackupState, LiveIdentity, apply_shared_document, project_shared_document, stamp_identity,
    tombstone_identity,
};
use crate::bunker::state::BunkerData;
use crate::crypto::{self, KEY_LEN, KdfParams, SecretKeyBytes};
use crate::error::{Error, Result, check_password};
use crate::identity::{self, BrowserLink, Custody, IdentitySummary, NewKey, StoredIdentity};
use crate::time;

mod desktop;
pub(crate) use desktop::DesktopRecord;

const FORMAT: &str = "gooti-desktop-vault";
const FILE_VERSION: u32 = 1;
const DATA_VERSION: u32 = 1;
const SALT_LEN: usize = 16;
const AAD_KEY: &[u8] = b"gooti-desktop-vault/v1/data-key";
const AAD_DATA: &[u8] = b"gooti-desktop-vault/v1/data";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum VaultStatus {
    NoVault,
    Locked,
    Unlocked,
}

/// What the UI may know about backup. No keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub relays: Vec<String>,
    pub auto_backup: bool,
    /// Base64 characters the sealed vault takes on a relay, out of `BACKUP_CONTENT_LIMIT`.
    pub content_chars: usize,
    /// The relays hold everything this desktop keeps in the desktop record: a reset loses
    /// nothing that "Restore from backup" cannot bring back.
    pub desktop_current: bool,
}

/// What applying a merged document did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// The vault changed while the relays were busy. Nothing was applied; backup again.
    Stale,
    /// Applied. Nothing more to send.
    Done,
    /// Applied, and this desktop changed it again: a Desktop only key came back in the merge
    /// and was replaced by its pointer. Backup once more to send that.
    SendAgain,
}

/// Everything a backup needs from the vault, taken under the lock so the network part runs without it.
pub struct SharedPlan {
    pub keys: BackupKeys,
    pub relays: Vec<String>,
    /// The live identities projected over the stored stamps.
    pub document: SharedDocument,
    /// The vault's change counter when the plan was made.
    pub revision: u64,
}

/// The file on disk. Only `kdf` and the salt are in the clear.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VaultFile {
    format: String,
    version: u32,
    kdf: KdfParams,
    salt: String,
    /// The data key, sealed with the password key.
    data_key: String,
    /// `VaultData` as JSON, sealed with the data key.
    data: String,
}

/// The sealed contents.
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VaultData {
    version: u32,
    pub(crate) identities: Vec<StoredIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    backup: Option<BackupState>,
    /// This desktop's bunker. Most of it also goes into the desktop record.
    #[serde(default)]
    pub(crate) bunker: BunkerData,
    /// Made once per vault. Says which desktop answers on a signer key.
    #[serde(default)]
    pub(crate) desktop_id: String,
    /// The desktop record, while backup is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) desktop_record: Option<DesktopRecord>,
    /// What happened on this desktop. Never backed up.
    #[serde(default)]
    pub(crate) activity: Vec<ActivityEntry>,
}

pub(crate) struct Unlocked {
    data_key: SecretKeyBytes,
    pub(crate) data: VaultData,
    /// Activity recorded since the last save.
    activity_dirty: bool,
    /// Counts saved changes in this session. A backup applies its result only if this did not move.
    revision: u64,
}

pub struct Vault {
    path: PathBuf,
    kdf: KdfParams,
    unlocked: Option<Unlocked>,
}

impl Vault {
    /// A vault stored at `path`. Nothing is read until `unlock`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self::with_kdf(path, KdfParams::DEFAULT)
    }

    /// Like `new`, with the Argon2id cost used for a new vault or a password change.
    pub fn with_kdf(path: impl Into<PathBuf>, kdf: KdfParams) -> Self {
        Vault {
            path: path.into(),
            kdf,
            unlocked: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn status(&self) -> VaultStatus {
        if self.unlocked.is_some() {
            VaultStatus::Unlocked
        } else if self.path.exists() {
            VaultStatus::Locked
        } else {
            VaultStatus::NoVault
        }
    }

    pub fn is_unlocked(&self) -> bool {
        self.unlocked.is_some()
    }

    /// Creates an empty vault and leaves it unlocked.
    pub fn create(&mut self, password: &str) -> Result<()> {
        check_password(password)?;
        if self.path.exists() {
            return Err(Error::VaultExists);
        }
        let unlocked = Unlocked {
            data_key: crypto::random_bytes::<KEY_LEN>(),
            data: VaultData {
                version: DATA_VERSION,
                identities: Vec::new(),
                backup: None,
                bunker: BunkerData::default(),
                desktop_id: uuid::Uuid::new_v4().to_string(),
                desktop_record: None,
                activity: Vec::new(),
            },
            activity_dirty: false,
            revision: 0,
        };
        let file = self.seal_new_file(password, &unlocked)?;
        write_atomic(&self.path, &file_bytes(&file))?;
        self.unlocked = Some(unlocked);
        Ok(())
    }

    pub fn unlock(&mut self, password: &str) -> Result<()> {
        let file = self.read_file()?;
        let salt = crypto::from_base64(&file.salt)?;
        let password_key = crypto::derive_key(password, &salt, file.kdf)?;
        let data_key = open_data_key(&password_key, &file)?.ok_or(Error::InvalidPassword)?;
        self.finish_unlock(&file, data_key)
    }

    /// Unlocks with a data key from the OS keychain ("Unlock at login").
    pub fn unlock_with_data_key(&mut self, data_key: &[u8; KEY_LEN]) -> Result<()> {
        let file = self.read_file()?;
        self.finish_unlock(&file, Zeroizing::new(*data_key))
    }

    /// The data key while unlocked, to store in the OS keychain.
    pub fn data_key(&self) -> Result<SecretKeyBytes> {
        Ok(self.unlocked()?.data_key.clone())
    }

    /// Forgets the data key and every private key held in memory.
    /// Activity recorded since the last save is saved first.
    pub fn lock(&mut self) {
        let _ = self.flush_activity();
        self.unlocked = None;
    }

    /// Records what happened, in memory. `flush_activity` (or any save, or lock) writes it.
    pub fn record_activity(&mut self, entry: ActivityEntry) {
        if let Some(unlocked) = self.unlocked.as_mut() {
            activity::push(&mut unlocked.data.activity, entry);
            unlocked.activity_dirty = true;
        }
    }

    /// Saves recorded activity, if there is any.
    pub fn flush_activity(&mut self) -> Result<()> {
        if self.unlocked.as_ref().is_some_and(|u| u.activity_dirty) {
            self.save()?;
        }
        Ok(())
    }

    /// Newest first.
    pub fn activity(&self) -> Result<Vec<ActivityEntry>> {
        Ok(self
            .unlocked()?
            .data
            .activity
            .iter()
            .rev()
            .cloned()
            .collect())
    }

    pub fn clear_activity(&mut self) -> Result<()> {
        self.unlocked_mut()?.data.activity.clear();
        self.save()
    }

    /// Deletes the vault file. Everything on this desktop is gone.
    pub fn reset(&mut self) -> Result<()> {
        self.lock();
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Needs the current password. The data key stays the same, so a keychain entry stays valid.
    pub fn change_password(&mut self, current: &str, new_password: &str) -> Result<()> {
        check_password(new_password)?;
        let file = self.read_file()?;
        let salt = crypto::from_base64(&file.salt)?;
        let current_key = crypto::derive_key(current, &salt, file.kdf)?;
        open_data_key(&current_key, &file)?.ok_or(Error::InvalidPassword)?;
        let unlocked = self.unlocked()?;
        let file = self.seal_new_file(new_password, unlocked)?;
        write_atomic(&self.path, &file_bytes(&file))
    }

    /// Counts changes to identities and backup since unlock. A backup applies its result only if
    /// this did not move while it waited on the relays.
    pub fn revision(&self) -> Result<u64> {
        Ok(self.unlocked()?.revision)
    }

    // Identities

    pub fn identities(&self) -> Result<Vec<IdentitySummary>> {
        let unlocked = self.unlocked()?;
        Ok(unlocked
            .data
            .identities
            .iter()
            .map(|identity| summary_of(unlocked, identity))
            .collect())
    }

    pub fn identity(&self, id: &str) -> Result<IdentitySummary> {
        Ok(summary_of(self.unlocked()?, self.find(id)?))
    }

    /// Adds an identity whose key backup carries (`Custody::Shared`).
    pub fn add_identity(&mut self, nick: &str, key: NewKey<'_>) -> Result<IdentitySummary> {
        self.add_identity_as(nick, key, Custody::Shared)
    }

    /// Adds an identity with the chosen custody. `Desktop` keeps the key off the backup document;
    /// browsers reach it on the default relays.
    pub fn add_identity_as(
        &mut self,
        nick: &str,
        key: NewKey<'_>,
        custody: Custody,
    ) -> Result<IdentitySummary> {
        self.add_identity_on(nick, key, custody, None)
    }

    /// Like `add_identity_as`, with the relays browsers reach a Desktop only identity on
    /// (`None`: the default relays).
    pub fn add_identity_on(
        &mut self,
        nick: &str,
        key: NewKey<'_>,
        custody: Custody,
        relays: Option<&[String]>,
    ) -> Result<IdentitySummary> {
        let browser_relays = if custody == Custody::Desktop {
            Some(self.browser_relays_or_default(relays)?)
        } else {
            None
        };
        let nick = identity::normalize_nick(nick)?;
        let keys = identity::keys_for(key)?;
        let pubkey = keys.public_key().to_hex();
        let unlocked = self.unlocked_mut()?;
        if let Some(existing) = unlocked.data.identities.iter().find(|i| i.pubkey == pubkey) {
            return Err(Error::DuplicateIdentity(existing.nick.clone()));
        }
        let stored = StoredIdentity {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: time::iso_from_ms(time::now_ms()),
            nick,
            pubkey,
            privkey: Some(keys.secret_key().to_secret_hex()),
            color: None,
            bunker: None,
            custody: Custody::Shared,
            custody_at: None,
            browsers: None,
            deleted_at: None,
        };
        let id = stored.id.clone();
        unlocked.data.identities.push(stored);
        if let Some(relays) = browser_relays {
            // A new key: no desktop serves it yet, so this one does.
            self.ensure_browser_link(&id, relays)?;
            let identity = self.identity_mut(&id)?;
            identity.custody = Custody::Desktop;
            identity.custody_at = Some(now());
        }
        let unlocked = self.unlocked_mut()?;
        let index = unlocked.data.identities.len() - 1;
        stamp(unlocked, index);
        self.save_change()?;
        self.identity(&id)
    }

    /// Moves the private key to this desktop only, or back into backup.
    /// Desktop: backup carries a bunker pointer and browsers drop their copy at their next backup.
    /// Shared: the shared record carries the key again, to every device with the passphrase.
    pub fn set_custody(&mut self, id: &str, custody: Custody) -> Result<IdentitySummary> {
        self.set_custody_on(id, custody, None)
    }

    /// Like `set_custody`, with the relays browsers reach the identity on when it moves to the
    /// desktop (`None`: the default relays).
    pub fn set_custody_on(
        &mut self,
        id: &str,
        custody: Custody,
        relays: Option<&[String]>,
    ) -> Result<IdentitySummary> {
        if self.find(id)?.privkey.is_none() {
            return Err(Error::CannotSign);
        }
        if custody == Custody::Desktop {
            let relays = self.browser_relays_or_default(relays)?;
            self.ensure_browser_link(id, relays)?;
        } else {
            self.drop_browser_link(id)?;
        }
        self.update(id, |identity| {
            if identity.custody != custody {
                // Later than any custody time backup has seen, even from a clock that runs ahead.
                identity.custody_at =
                    Some(identity.custody_at.map_or(now(), |at| now().max(at + 1.0)));
            }
            identity.custody = custody;
        })
    }

    /// After the bunker relays changed: the pointers of Desktop only identities changed too.
    pub(crate) fn restamp_desktop_identities(&mut self) -> Result<()> {
        let unlocked = self.unlocked_mut()?;
        let indexes: Vec<usize> = unlocked
            .data
            .identities
            .iter()
            .enumerate()
            .filter(|(_, i)| i.custody == Custody::Desktop)
            .map(|(index, _)| index)
            .collect();
        if indexes.is_empty() {
            return Ok(());
        }
        for index in indexes {
            stamp(unlocked, index);
        }
        self.save_change()
    }

    pub(crate) fn identity_mut(&mut self, id: &str) -> Result<&mut StoredIdentity> {
        self.unlocked_mut()?
            .data
            .identities
            .iter_mut()
            .find(|identity| identity.id == id)
            .ok_or(Error::IdentityNotFound)
    }

    pub fn rename_identity(&mut self, id: &str, nick: &str) -> Result<IdentitySummary> {
        let nick = identity::normalize_nick(nick)?;
        self.update(id, |identity| identity.nick = nick)
    }

    /// `None` goes back to the color derived from the public key.
    pub fn set_identity_color(&mut self, id: &str, color: Option<&str>) -> Result<IdentitySummary> {
        let color = color.map(identity::normalize_color).transpose()?;
        self.update(id, |identity| identity.color = color)
    }

    /// With backup on, the backup document keeps a tombstone so other devices delete it too.
    pub fn delete_identity(&mut self, id: &str) -> Result<()> {
        let unlocked = self.unlocked_mut()?;
        let at = unlocked
            .data
            .identities
            .iter()
            .position(|identity| identity.id == id)
            .ok_or(Error::IdentityNotFound)?;
        // The tombstone carries what backup carried: the key, or for Desktop only the pointer.
        let tombstone = live_of(unlocked, &unlocked.data.identities[at]);
        let removed = unlocked.data.identities.remove(at);
        unlocked.data.bunker.forget_identity(&removed.pubkey);
        if let (Some(backup), Some(tombstone)) = (unlocked.data.backup.as_mut(), tombstone) {
            tombstone_identity(&mut backup.document, &tombstone, now());
        }
        self.save_change()
    }

    /// The nsec, for an explicit "Show" action. The only way a private key reaches the UI.
    pub fn reveal_nsec(&self, id: &str) -> Result<Zeroizing<String>> {
        Ok(identity::nsec(&self.find(id)?.keys()?))
    }

    pub fn export_ncryptsec(&self, id: &str, password: &str) -> Result<String> {
        identity::ncryptsec(&self.find(id)?.keys()?, password)
    }

    // Backup

    /// `None` while backup is off.
    pub fn backup_info(&self) -> Result<Option<BackupInfo>> {
        let unlocked = self.unlocked()?;
        Ok(unlocked.data.backup.as_ref().map(|backup| {
            let (document, _) = project_shared_document(&backup.document, &live(unlocked), now());
            BackupInfo {
                relays: backup.relays.clone(),
                auto_backup: backup.auto_backup,
                content_chars: shared_content_chars(&document),
                desktop_current: self.desktop_current(),
            }
        }))
    }

    /// Turns backup on with a new passphrase. Nothing is sent until the next backup.
    pub fn enable_backup(&mut self, phrase: &str, relays: &[String]) -> Result<()> {
        let relays = normalize_relays(relays)?;
        let keys = derive_shared_keys(phrase)?;
        let desktop_keys = derive_desktop_keys(phrase)?;
        let unlocked = self.unlocked_mut()?;
        if unlocked.data.backup.is_some() {
            return Err(Error::BackupOn);
        }
        let (document, _) =
            project_shared_document(&SharedDocument::default(), &live(unlocked), now());
        unlocked.data.backup = Some(new_backup_state(&keys, relays, document));
        unlocked.data.desktop_record = Some(DesktopRecord::new(&desktop_keys));
        self.save_change()
    }

    /// Stops backing up on this desktop. Identities stay; the relay copy is left as it is.
    pub fn disable_backup(&mut self) -> Result<()> {
        let unlocked = self.unlocked_mut()?;
        unlocked.data.backup = None;
        unlocked.data.desktop_record = None;
        unlocked.data.bunker.removed.clear();
        self.save_change()
    }

    pub fn set_backup_relays(&mut self, relays: &[String]) -> Result<Vec<String>> {
        let relays = normalize_relays(relays)?;
        self.backup_state_mut()?.relays = relays.clone();
        self.save()?;
        Ok(relays)
    }

    pub fn set_auto_backup(&mut self, on: bool) -> Result<()> {
        self.backup_state_mut()?.auto_backup = on;
        self.save()
    }

    pub fn auto_backup(&self) -> bool {
        self.unlocked
            .as_ref()
            .and_then(|u| u.data.backup.as_ref())
            .is_some_and(|backup| backup.auto_backup)
    }

    /// Projects the live identities over the stamps, keeps those stamps, and returns what a
    /// backup needs. The relays are then read and written without holding the vault.
    pub fn shared_plan(&mut self) -> Result<SharedPlan> {
        let unlocked = self.unlocked_mut()?;
        let live = live(unlocked);
        let backup = unlocked.data.backup.as_mut().ok_or(Error::BackupOff)?;
        let (document, changed) = project_shared_document(&backup.document, &live, now());
        let keys = BackupKeys::from_hex(&backup.signing_key_hex, &backup.encryption_key_hex)?;
        let relays = backup.relays.clone();
        if changed {
            backup.document = document.clone();
            self.save()?;
        }
        Ok(SharedPlan {
            keys,
            relays,
            document,
            revision: self.revision()?,
        })
    }

    /// For joining with an existing passphrase: this desktop's identities as a fresh document.
    pub fn join_plan(&self) -> Result<(SharedDocument, u64)> {
        let unlocked = self.unlocked()?;
        if unlocked.data.backup.is_some() {
            return Err(Error::BackupOn);
        }
        let (document, _) =
            project_shared_document(&SharedDocument::default(), &live(unlocked), now());
        Ok((document, unlocked.revision))
    }

    /// Stores a merged document and rebuilds the identities from it.
    /// `Stale` changes nothing: the vault changed since `revision`, so the caller backups again
    /// and the newer edit is not lost.
    pub fn finish_shared(&mut self, revision: u64, merged: SharedDocument) -> Result<Applied> {
        let unlocked = self.unlocked_mut()?;
        if unlocked.revision != revision {
            return Ok(Applied::Stale);
        }
        let backup = unlocked.data.backup.as_mut().ok_or(Error::BackupOff)?;
        backup.document = merged;
        let applied = apply_merged(unlocked);
        self.save_change()?;
        Ok(applied)
    }

    /// Like `finish_shared`, and turns backup on with these keys and relays. The desktop record
    /// starts empty; the next backup merges it with the relays.
    pub fn finish_join(
        &mut self,
        revision: u64,
        keys: &BackupKeys,
        desktop_keys: &BackupKeys,
        relays: Vec<String>,
        merged: SharedDocument,
    ) -> Result<Applied> {
        let unlocked = self.unlocked_mut()?;
        if unlocked.data.backup.is_some() {
            return Err(Error::BackupOn);
        }
        if unlocked.revision != revision {
            return Ok(Applied::Stale);
        }
        unlocked.data.backup = Some(new_backup_state(keys, relays, merged));
        unlocked.data.desktop_record = Some(DesktopRecord::new(desktop_keys));
        let applied = apply_merged(unlocked);
        self.save_change()?;
        Ok(applied)
    }

    // Internals

    fn update(
        &mut self,
        id: &str,
        change: impl FnOnce(&mut StoredIdentity),
    ) -> Result<IdentitySummary> {
        let unlocked = self.unlocked_mut()?;
        let index = unlocked
            .data
            .identities
            .iter()
            .position(|identity| identity.id == id)
            .ok_or(Error::IdentityNotFound)?;
        change(&mut unlocked.data.identities[index]);
        let summary = unlocked.data.identities[index].summary();
        stamp(unlocked, index);
        self.save_change()?;
        Ok(summary)
    }

    pub(crate) fn find(&self, id: &str) -> Result<&StoredIdentity> {
        self.unlocked()?
            .data
            .identities
            .iter()
            .find(|identity| identity.id == id)
            .ok_or(Error::IdentityNotFound)
    }

    fn backup_state_mut(&mut self) -> Result<&mut BackupState> {
        self.unlocked_mut()?
            .data
            .backup
            .as_mut()
            .ok_or(Error::BackupOff)
    }

    /// Saves a change that a running backup must not overwrite.
    fn save_change(&mut self) -> Result<()> {
        self.unlocked_mut()?.revision += 1;
        self.save()
    }

    pub(crate) fn unlocked(&self) -> Result<&Unlocked> {
        self.unlocked.as_ref().ok_or(Error::Locked)
    }

    pub(crate) fn unlocked_mut(&mut self) -> Result<&mut Unlocked> {
        self.unlocked.as_mut().ok_or(Error::Locked)
    }

    fn read_file(&self) -> Result<VaultFile> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::NoVault);
            }
            Err(error) => return Err(error.into()),
        };
        let file: VaultFile = serde_json::from_slice(&bytes).map_err(|_| Error::UnreadableVault)?;
        if file.format != FORMAT || file.version != FILE_VERSION {
            return Err(Error::UnreadableVault);
        }
        Ok(file)
    }

    fn finish_unlock(&mut self, file: &VaultFile, data_key: SecretKeyBytes) -> Result<()> {
        let sealed = crypto::from_base64(&file.data)?;
        let plaintext = crypto::open(&data_key, &sealed, AAD_DATA).ok_or(Error::InvalidPassword)?;
        let mut data: VaultData =
            serde_json::from_slice(&plaintext).map_err(|_| Error::UnreadableVault)?;
        if data.version != DATA_VERSION {
            return Err(Error::UnreadableVault);
        }
        // The first alpha kept deleted identities as rows of their own.
        data.identities
            .retain(|identity| identity.deleted_at.is_none());
        let new_id = data.desktop_id.is_empty();
        if new_id {
            data.desktop_id = uuid::Uuid::new_v4().to_string();
        }
        // Before 2026-11-12, apps from a `bunker://` link kept no relays of their own.
        let pinned = data.bunker.pin_app_relays();
        self.unlocked = Some(Unlocked {
            data_key,
            data,
            activity_dirty: false,
            revision: 0,
        });
        if new_id || pinned {
            self.save()?;
        }
        Ok(())
    }

    /// Writes a file with a fresh salt, sealed for `password`.
    fn seal_new_file(&self, password: &str, unlocked: &Unlocked) -> Result<VaultFile> {
        let salt = crypto::random_bytes::<SALT_LEN>();
        let password_key = crypto::derive_key(password, salt.as_ref(), self.kdf)?;
        Ok(VaultFile {
            format: FORMAT.to_string(),
            version: FILE_VERSION,
            kdf: self.kdf,
            salt: crypto::to_base64(salt.as_ref()),
            data_key: crypto::to_base64(&crypto::seal(
                &password_key,
                unlocked.data_key.as_ref(),
                AAD_KEY,
            )),
            data: seal_data(unlocked),
        })
    }

    /// Re-seals the data and keeps the password part of the file.
    pub(crate) fn save(&mut self) -> Result<()> {
        let mut file = self.read_file()?;
        file.data = seal_data(self.unlocked()?);
        write_atomic(&self.path, &file_bytes(&file))?;
        self.unlocked_mut()?.activity_dirty = false;
        Ok(())
    }
}

fn summary_of(unlocked: &Unlocked, identity: &StoredIdentity) -> IdentitySummary {
    let mut summary = identity.summary();
    summary.served_elsewhere = Vault::served_elsewhere(unlocked, &identity.pubkey);
    summary
}

fn live(unlocked: &Unlocked) -> Vec<LiveIdentity> {
    unlocked
        .data
        .identities
        .iter()
        .filter_map(|identity| live_of(unlocked, identity))
        .collect()
}

/// An identity as backup carries it. For Desktop only: the bunker pointer instead of the key.
/// `None` when that pointer cannot be built (no `wss://` bunker relay): the identity is then
/// left out of the document rather than sent with its key.
pub(crate) fn live_of(unlocked: &Unlocked, identity: &StoredIdentity) -> Option<LiveIdentity> {
    let mut live = identity.live();
    if identity.custody == Custody::Desktop && identity.privkey.is_some() {
        live.privkey = None;
        live.bunker = Some(crate::bunker::store::browser_pointer(
            &unlocked.data.bunker,
            identity,
        )?);
    }
    Some(live)
}

/// With backup on, records a change to the identity at `index` in the backup document: at now, and
/// always later than the row it replaces, so an edit wins even over a clock that runs ahead.
fn stamp(unlocked: &mut Unlocked, index: usize) {
    let pubkey = &unlocked.data.identities[index].pubkey;
    let previous = unlocked.data.backup.as_ref().and_then(|backup| {
        backup
            .document
            .identities
            .iter()
            .find(|row| &row.pubkey == pubkey)
            .map(|row| row.updated_at.max(row.deleted_at.unwrap_or(0.0)))
    });
    let at = previous.map_or(now(), |previous| now().max(previous + 1.0));
    stamp_at(unlocked, index, at);
}

fn stamp_at(unlocked: &mut Unlocked, index: usize, at: f64) {
    let Some(live) = live_of(unlocked, &unlocked.data.identities[index]) else {
        return;
    };
    if let Some(backup) = unlocked.data.backup.as_mut() {
        stamp_identity(&mut backup.document, &live, at);
    }
}

/// Rebuilds the identities from the stored backup document.
fn apply_merged(unlocked: &mut Unlocked) -> Applied {
    let Some(backup) = unlocked.data.backup.as_ref() else {
        return Applied::Done;
    };
    // Rows to send again, with the revision of the row that won, so ours can be stamped later:
    // - a Desktop only key that came back in the merge (a browser imported it again);
    // - a key this desktop holds whose winning row has an older custody time than ours, or none
    //   (a browser before 2.1 edited it and dropped the field). Sending ours again restores it.
    let again_for: Vec<(String, f64)> = unlocked
        .data
        .identities
        .iter()
        .filter(|i| i.privkey.is_some())
        .filter_map(|i| {
            let row = backup
                .document
                .identities
                .iter()
                .find(|row| row.pubkey == i.pubkey && row.deleted_at.is_none())?;
            // A key that came back with a newer custody time was moved on purpose, by another
            // Gooti Desktop ("Use in browsers again"): this desktop follows instead.
            let moved_on_purpose = row
                .custody_at
                .is_some_and(|theirs| i.custody_at.is_none_or(|ours| theirs > ours));
            let returned =
                i.custody == Custody::Desktop && row.privkey.is_some() && !moved_on_purpose;
            let older_custody = i
                .custody_at
                .is_some_and(|ours| row.custody_at.is_none_or(|theirs| theirs < ours));
            (returned || older_custody).then(|| (i.pubkey.clone(), row.updated_at))
        })
        .collect();
    let applied = apply_shared_document(
        &unlocked.data.identities,
        &backup.document,
        |identity| identity.pubkey.as_str(),
        |identity| identity.bunker.as_ref(),
        |identity| identity.id.clone(),
        || uuid::Uuid::new_v4().to_string(),
    );
    let signers = known_signers(unlocked);
    let mut previous = std::mem::take(&mut unlocked.data.identities);
    unlocked.data.identities = applied
        .into_iter()
        .map(|(id, mut live)| {
            let before = previous.iter_mut().find(|p| p.pubkey == live.pubkey);
            let winning_pointer = live.bunker.clone();
            let winning_has_key = live.privkey.is_some();
            // Never drop a private key this desktop holds for a bunker pointer from another
            // device. That pointer may well point at this desktop's own bunker, and this copy
            // may be the only one. (The extension drops it; the desktop is the key holder.)
            if live.privkey.is_none()
                && let Some(kept) = before.as_ref().and_then(|p| p.privkey.clone())
            {
                live.privkey = Some(kept);
                live.bunker = None;
            }
            let mut stored = StoredIdentity::from_live(id, live);
            // Custody and the browser session key are this desktop's own; backup does not carry them.
            if let Some(before) = before
                && stored.privkey.is_some()
            {
                stored.custody = before.custody;
                stored.browsers = before.browsers.take();
                // Another Gooti Desktop moved the key later than this one did: follow it.
                let newer = stored
                    .custody_at
                    .is_some_and(|theirs| before.custody_at.is_none_or(|ours| theirs > ours));
                if newer {
                    match &winning_pointer {
                        Some(pointer)
                            if signers
                                .contains(&(pointer.pubkey.clone(), stored.pubkey.clone())) =>
                        {
                            if let Some(client) = pointer.client_secret_key.clone() {
                                stored.custody = Custody::Desktop;
                                stored.browsers = Some(BrowserLink {
                                    client_secret_hex: client,
                                    ask_site: pointer.ask_site,
                                    signer_pubkey: Some(pointer.pubkey.clone()),
                                });
                            }
                        }
                        None if winning_has_key => {
                            stored.custody = Custody::Shared;
                            stored.browsers = None;
                        }
                        _ => {}
                    }
                }
                // A newer custody time from backup is kept; an older one does not replace ours.
                stored.custody_at = match (stored.custody_at, before.custody_at) {
                    (Some(theirs), Some(ours)) => Some(theirs.max(ours)),
                    (theirs, ours) => theirs.or(ours),
                };
            }
            stored
        })
        .collect();
    // An identity deleted on another device takes its bunker connections with it.
    let kept: Vec<String> = unlocked
        .data
        .identities
        .iter()
        .map(|i| i.pubkey.clone())
        .collect();
    unlocked.data.bunker.keep_identities(&kept);
    // The browsers' connection belongs to a Desktop only identity.
    let not_desktop: Vec<String> = unlocked
        .data
        .identities
        .iter()
        .filter(|i| i.custody != Custody::Desktop)
        .map(|i| i.pubkey.clone())
        .collect();
    let bunker = &mut unlocked.data.bunker;
    let ended: Vec<String> = bunker
        .connections
        .iter()
        .filter(|c| c.browsers && not_desktop.contains(&c.identity_pubkey))
        .map(|c| c.id.clone())
        .collect();
    bunker.connections.retain(|c| !ended.contains(&c.id));
    bunker.removed.extend(ended);
    // Keys for pointers that just arrived, from the desktop record.
    desktop::hydrate(unlocked, false);
    let restamped = desktop::restamp_pointers(unlocked);

    // Send the pointer again, newer than the key that came back, even when that device's clock
    // runs ahead of this one.
    let mut again = false;
    for (pubkey, revision) in again_for {
        if let Some(index) = unlocked
            .data
            .identities
            .iter()
            .position(|i| i.pubkey == pubkey)
        {
            stamp_at(unlocked, index, now().max(revision + 1.0));
            again = true;
        }
    }
    if again || restamped {
        Applied::SendAgain
    } else {
        Applied::Done
    }
}

/// (signer pubkey, identity pubkey) of every signer key this desktop knows, here or in the
/// desktop record.
fn known_signers(unlocked: &Unlocked) -> Vec<(String, String)> {
    let mut known: Vec<(String, String)> = unlocked
        .data
        .bunker
        .signers
        .iter()
        .filter_map(|s| {
            crate::bunker::store::keys_from_hex(&s.secret_hex)
                .ok()
                .map(|k| (k.public_key().to_hex(), s.identity_pubkey.clone()))
        })
        .collect();
    if let Some(backup) = unlocked.data.desktop_record.as_ref() {
        for (pubkey, row) in &backup.document.signers {
            if let Some(signer) = row.live() {
                known.push((pubkey.clone(), signer.identity_pubkey.clone()));
            }
        }
    }
    known
}

fn new_backup_state(
    keys: &BackupKeys,
    relays: Vec<String>,
    document: SharedDocument,
) -> BackupState {
    BackupState {
        signing_key_hex: keys.signing_hex().to_string(),
        encryption_key_hex: keys.encryption_hex().to_string(),
        relays,
        document,
        auto_backup: false,
    }
}

/// Trimmed, without blanks or repeats, each a `wss://` URL. Same rule as the extension.
pub fn normalize_relays(relays: &[String]) -> Result<Vec<String>> {
    let mut normalized: Vec<String> = Vec::new();
    for relay in relays {
        let url = relay.trim();
        if url.is_empty() {
            continue;
        }
        if !is_wss_url(url) {
            return Err(Error::InvalidRelay);
        }
        if !normalized.iter().any(|seen| seen == url) {
            normalized.push(url.to_string());
        }
    }
    if normalized.is_empty() {
        return Err(Error::NoRelays);
    }
    Ok(normalized)
}

fn now() -> f64 {
    time::now_ms() as f64
}

fn file_bytes(file: &VaultFile) -> Vec<u8> {
    serde_json::to_vec_pretty(file).expect("vault file serializes")
}

fn open_data_key(password_key: &[u8; KEY_LEN], file: &VaultFile) -> Result<Option<SecretKeyBytes>> {
    let sealed = crypto::from_base64(&file.data_key)?;
    let Some(bytes) = crypto::open(password_key, &sealed, AAD_KEY) else {
        return Ok(None);
    };
    let key: [u8; KEY_LEN] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::UnreadableVault)?;
    Ok(Some(Zeroizing::new(key)))
}

fn seal_data(unlocked: &Unlocked) -> String {
    let plaintext =
        Zeroizing::new(serde_json::to_vec(&unlocked.data).expect("vault data serializes"));
    crypto::to_base64(&crypto::seal(&unlocked.data_key, &plaintext, AAD_DATA))
}

/// Write to a temporary file in the same folder, flush it to disk, then rename it over the
/// target. A crash leaves either the old file or the new one, never half of each.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let tmp = path.with_extension("tmp");
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    #[cfg(unix)]
    fs::File::open(dir)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests;
