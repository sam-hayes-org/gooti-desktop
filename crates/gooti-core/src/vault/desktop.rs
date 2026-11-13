//! The desktop record on this desktop: what goes into it, and what comes back from it.
//! The format and the merge are in `backup::desktop`.
//!
//! Rows are only ever deleted for a reason this desktop knows: a connection it ended, or an
//! identity the shared record deleted. An identity that is merely missing here (its shared row
//! has not arrived yet) keeps its rows, so a backup that fails halfway never deletes a key.

use serde::{Deserialize, Serialize};

use super::{Applied, Unlocked, Vault, live_of, now, stamp};
use crate::backup::desktop::{DesktopDocument, Row, SignerRow, bury, put};
use crate::backup::phrase::BackupKeys;
use crate::bunker::state::SignerKey;
use crate::bunker::store::keys_from_hex;
use crate::error::{Error, Result};
use crate::identity::{BrowserLink, Custody};

/// The desktop record's keys and the last document merged with the relays.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopRecord {
    signing_key_hex: String,
    encryption_key_hex: String,
    pub(crate) document: DesktopDocument,
    /// The document holds changes no relay has confirmed yet.
    #[serde(default)]
    unsent: bool,
}

impl DesktopRecord {
    pub(crate) fn new(keys: &BackupKeys) -> Self {
        DesktopRecord {
            signing_key_hex: keys.signing_hex().to_string(),
            encryption_key_hex: keys.encryption_hex().to_string(),
            document: DesktopDocument::default(),
            unsent: false,
        }
    }
}

/// What a backup of the desktop record needs, taken under the lock.
pub struct DesktopPlan {
    pub keys: BackupKeys,
    pub relays: Vec<String>,
    /// This desktop's state written over the stored document.
    pub document: DesktopDocument,
}

impl Vault {
    /// `None` while backup is off. Stores the projected document, like `shared_plan`.
    pub fn desktop_plan(&mut self) -> Result<Option<DesktopPlan>> {
        let unlocked = self.unlocked_mut()?;
        let Some(backup) = unlocked.data.desktop_record.as_ref() else {
            return Ok(None);
        };
        let relays = unlocked
            .data
            .backup
            .as_ref()
            .ok_or(Error::BackupOff)?
            .relays
            .clone();
        let keys = BackupKeys::from_hex(&backup.signing_key_hex, &backup.encryption_key_hex)?;
        let (document, changed) = project(unlocked, &backup.document, now());
        let had_removed = !unlocked.data.bunker.removed.is_empty();
        unlocked.data.bunker.removed.clear();
        if changed {
            let backup = unlocked
                .data
                .desktop_record
                .as_mut()
                .expect("checked above");
            backup.document = document.clone();
            backup.unsent = true;
        }
        if changed || had_removed {
            self.save()?;
        }
        Ok(Some(DesktopPlan {
            keys,
            relays,
            document,
        }))
    }

    /// Stores a merged desktop record and takes in what it brings. `Stale` changes nothing:
    /// something this desktop keeps there changed while the relays were busy.
    /// `unsent` is true when the relays do not have `merged` yet.
    pub fn finish_desktop(
        &mut self,
        planned: &DesktopDocument,
        merged: DesktopDocument,
        unsent: bool,
    ) -> Result<Applied> {
        let unlocked = self.unlocked_mut()?;
        let backup = unlocked
            .data
            .desktop_record
            .as_ref()
            .ok_or(Error::BackupOff)?;
        let (_, changed) = project(unlocked, &backup.document, now());
        if changed || backup.document != *planned || !unlocked.data.bunker.removed.is_empty() {
            return Ok(Applied::Stale);
        }
        let backup = unlocked
            .data
            .desktop_record
            .as_mut()
            .expect("checked above");
        backup.document = merged;
        backup.unsent = unsent;
        hydrate(unlocked, true);
        // An older Gooti Desktop may still send apps without relays.
        unlocked.data.bunker.pin_app_relays();
        let stamped = restamp_pointers(unlocked);
        self.save_change()?;
        Ok(if stamped {
            Applied::SendAgain
        } else {
            Applied::Done
        })
    }

    /// True when the relays hold everything this desktop would put in the desktop record.
    pub(crate) fn desktop_current(&self) -> bool {
        let Ok(unlocked) = self.unlocked() else {
            return false;
        };
        let Some(backup) = unlocked.data.desktop_record.as_ref() else {
            return false;
        };
        !backup.unsent
            && unlocked.data.bunker.removed.is_empty()
            && !project(unlocked, &backup.document, now()).1
    }

    /// "Sign here instead": this desktop answers apps and browsers for these identities. The
    /// desktop that did so before stops at its next backup.
    pub fn serve_here(&mut self, identity_ids: &[String]) -> Result<()> {
        let mut pubkeys = Vec::new();
        for id in identity_ids {
            pubkeys.push(self.find(id)?.pubkey.clone());
        }
        let unlocked = self.unlocked_mut()?;
        for signer in &mut unlocked.data.bunker.signers {
            if pubkeys.contains(&signer.identity_pubkey) {
                signer.served_by = None;
            }
        }
        self.save()
    }

    /// Another Gooti Desktop answers for this identity, not this one.
    pub(crate) fn served_elsewhere(unlocked: &Unlocked, identity_pubkey: &str) -> bool {
        let data = &unlocked.data;
        let Some(identity) = data
            .identities
            .iter()
            .find(|i| i.pubkey == identity_pubkey && i.privkey.is_some())
        else {
            return false;
        };
        let named = identity
            .browsers
            .as_ref()
            .filter(|_| identity.custody == Custody::Desktop)
            .and_then(|b| b.signer_pubkey.clone());
        let relevant: Vec<&SignerKey> = data
            .bunker
            .signers
            .iter()
            .filter(|s| s.identity_pubkey == identity_pubkey)
            .filter(|s| {
                named.as_ref().is_none_or(|named| {
                    keys_from_hex(&s.secret_hex).is_ok_and(|k| k.public_key().to_hex() == *named)
                })
            })
            .collect();
        match &named {
            // The browsers' signer key decides for a Desktop only identity.
            Some(_) => relevant.iter().any(|s| !s.served_by(&data.desktop_id)),
            // Otherwise: it has signer keys, and this desktop serves none of them.
            None => !relevant.is_empty() && !relevant.iter().any(|s| s.served_by(&data.desktop_id)),
        }
    }
}

/// The stored document with this desktop's state written over it. True when that changed it.
pub(super) fn project(
    unlocked: &Unlocked,
    stored: &DesktopDocument,
    now: f64,
) -> (DesktopDocument, bool) {
    let data = &unlocked.data;
    let mut document = stored.clone();
    let mut changed = false;

    for identity in &data.identities {
        if let Some(privkey) = &identity.privkey {
            changed |= put(&mut document.keys, &identity.pubkey, privkey.clone(), now);
        }
    }
    for signer in &data.bunker.signers {
        let Ok(keys) = keys_from_hex(&signer.secret_hex) else {
            continue;
        };
        let row = SignerRow {
            identity_pubkey: signer.identity_pubkey.clone(),
            secret_hex: signer.secret_hex.clone(),
            served_by: signer
                .served_by
                .clone()
                .unwrap_or_else(|| data.desktop_id.clone()),
        };
        changed |= put(&mut document.signers, &keys.public_key().to_hex(), row, now);
    }
    for connection in &data.bunker.connections {
        let mut row = connection.clone();
        row.last_used_at = None;
        changed |= put(&mut document.connections, &connection.id, row, now);
    }
    for id in &data.bunker.removed {
        if !data.bunker.connections.iter().any(|c| &c.id == id) {
            changed |= bury(&mut document.connections, id, now);
        }
    }
    if let Some(at) = data.bunker.relays_at
        && document
            .bunker_relays
            .as_ref()
            .is_none_or(|row| row.at < at)
    {
        document.bunker_relays = Some(Row {
            at,
            value: Some(data.bunker.relays.clone()),
        });
        changed = true;
    }

    // An identity the shared record deleted takes its rows with it.
    if let Some(backup) = data.backup.as_ref() {
        for row in &backup.document.identities {
            if row.deleted_at.is_none() || data.identities.iter().any(|i| i.pubkey == row.pubkey) {
                continue;
            }
            changed |= bury(&mut document.keys, &row.pubkey, now);
            let signers: Vec<String> = document
                .signers
                .iter()
                .filter(|(_, s)| s.live().is_some_and(|s| s.identity_pubkey == row.pubkey))
                .map(|(key, _)| key.clone())
                .collect();
            for key in signers {
                changed |= bury(&mut document.signers, &key, now);
            }
            let connections: Vec<String> = document
                .connections
                .iter()
                .filter(|(_, c)| c.live().is_some_and(|c| c.identity_pubkey == row.pubkey))
                .map(|(key, _)| key.clone())
                .collect();
            for key in connections {
                changed |= bury(&mut document.connections, &key, now);
            }
        }
    }
    (document, changed)
}

/// Takes in the stored desktop record. Gives a Desktop only identity its key back when the
/// shared record has its pointer and this record has the key and the signer key it names.
/// `full` also replaces what this desktop holds with newer rows; without it, only what is
/// missing here is added, so a change made here since the last desktop backup stays.
pub(super) fn hydrate(unlocked: &mut Unlocked, full: bool) {
    let Some(backup) = unlocked.data.desktop_record.as_ref() else {
        return;
    };
    let document = backup.document.clone();
    let data = &mut unlocked.data;

    if let Some(row) = &document.bunker_relays
        && let Some(relays) = &row.value
        && data.bunker.relays_at.is_none_or(|at| at < row.at)
    {
        data.bunker.relays = relays.clone();
        data.bunker.relays_at = Some(row.at);
    }

    for identity in &mut data.identities {
        if identity.privkey.is_some() {
            continue;
        }
        let Some(pointer) = identity.bunker.as_ref() else {
            continue;
        };
        let Some(signer) = document.signer(&pointer.pubkey) else {
            continue;
        };
        let Some(privkey) = document.keys.get(&identity.pubkey).and_then(Row::live) else {
            continue;
        };
        let Some(client) = pointer.client_secret_key.clone() else {
            continue;
        };
        let matches =
            keys_from_hex(privkey).is_ok_and(|k| k.public_key().to_hex() == identity.pubkey);
        if signer.identity_pubkey != identity.pubkey || !matches {
            continue;
        }
        identity.browsers = Some(BrowserLink {
            client_secret_hex: client,
            ask_site: pointer.ask_site,
            signer_pubkey: Some(pointer.pubkey.clone()),
        });
        identity.privkey = Some(privkey.clone());
        identity.bunker = None;
        identity.custody = Custody::Desktop;
    }

    let holds: Vec<String> = data
        .identities
        .iter()
        .filter(|i| i.privkey.is_some())
        .map(|i| i.pubkey.clone())
        .collect();
    let bunker = &mut data.bunker;

    for (signer_pubkey, row) in &document.signers {
        match row.live() {
            Some(signer) if holds.contains(&signer.identity_pubkey) => {
                match bunker
                    .signers
                    .iter_mut()
                    .find(|s| s.secret_hex == signer.secret_hex)
                {
                    Some(local) if full => local.served_by = Some(signer.served_by.clone()),
                    Some(_) => {}
                    None => bunker.signers.push(SignerKey {
                        identity_pubkey: signer.identity_pubkey.clone(),
                        secret_hex: signer.secret_hex.clone(),
                        served_by: Some(signer.served_by.clone()),
                    }),
                }
            }
            None if full => bunker.signers.retain(|s| {
                keys_from_hex(&s.secret_hex)
                    .map_or(true, |k| k.public_key().to_hex() != *signer_pubkey)
            }),
            _ => {}
        }
    }

    for (id, row) in &document.connections {
        match row.live() {
            Some(connection)
                if holds.contains(&connection.identity_pubkey) && !bunker.removed.contains(id) =>
            {
                if let Some(local) = bunker.connections.iter_mut().find(|c| c.id == *id) {
                    if full {
                        let last_used_at = local.last_used_at;
                        *local = connection.clone();
                        local.last_used_at = last_used_at;
                    }
                    continue;
                }
                // The same app for the same identity under another id (two desktops made it):
                // the smaller id stays everywhere.
                let twin = bunker.connections.iter().position(|c| {
                    c.client_pubkey == connection.client_pubkey
                        && c.identity_pubkey == connection.identity_pubkey
                });
                match twin {
                    Some(at) if bunker.connections[at].id < *id => {}
                    Some(at) => {
                        let ours = bunker.connections.remove(at);
                        bunker.removed.push(ours.id);
                        bunker.connections.push(connection.clone());
                    }
                    None => bunker.connections.push(connection.clone()),
                }
            }
            None if full => bunker.connections.retain(|c| c.id != *id),
            _ => {}
        }
    }
}

/// Stamps a Desktop only identity whose pointer here differs from the shared row, so the
/// shared record gets the pointer this desktop would publish. True when it stamped one.
pub(super) fn restamp_pointers(unlocked: &mut Unlocked) -> bool {
    let Some(backup) = unlocked.data.backup.as_ref() else {
        return false;
    };
    let indexes: Vec<usize> = unlocked
        .data
        .identities
        .iter()
        .enumerate()
        .filter(|(_, i)| i.custody == Custody::Desktop && i.privkey.is_some())
        .filter(|(_, i)| {
            let Some(ours) = live_of(unlocked, i).and_then(|l| l.bunker) else {
                return false;
            };
            backup
                .document
                .identities
                .iter()
                .find(|row| row.pubkey == i.pubkey && row.deleted_at.is_none())
                .and_then(|row| row.bunker.as_ref())
                .is_some_and(|theirs| theirs.pubkey != ours.pubkey || theirs.relays != ours.relays)
        })
        .map(|(index, _)| index)
        .collect();
    for &index in &indexes {
        stamp(unlocked, index);
    }
    !indexes.is_empty()
}
