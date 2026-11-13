//! The backup flows, ported from `backup/manual.ts` and `storage/related/backup.ts`.
//!
//! The vault is locked only for the short steps before and after the network. The relays are
//! read before anything is published: publishing first would overwrite the relay copy before
//! this device could merge it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;

use super::blob::{open_shared_document, seal_shared_document};
use super::canonical::canonical_shared_document;
use super::changes::{ChangeReport, describe_changes};
use super::desktop::{
    DesktopDocument, merge_desktop_documents, open_desktop_document, seal_desktop_document,
};
use super::document::SharedDocument;
use super::event::{DESKTOP_D_TAG, SHARED_D_TAG, build_backup_event, read_backup_payload};
use super::merge::merge_shared_documents;
use super::phrase::{BackupKeys, derive_desktop_keys, derive_shared_keys};
use super::relay::RelayIo;
use crate::error::{Error, Result};
use crate::gooti::Gooti;
use crate::time;
use crate::vault::{Applied, normalize_relays};

/// A backup that keeps finding a newer edit gives up after this many rounds; the next one continues.
const MAX_ROUNDS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BackupResult {
    /// This desktop sent a backup.
    BackedUp,
    /// This desktop took the relay's changes and had nothing to send.
    Updated,
    /// Nothing changed.
    Current,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupOutcome {
    pub result: BackupResult,
    #[serde(flatten)]
    pub report: ChangeReport,
}

/// The newest vault on the relays, opened. `None` when there is none or it is an older format.
pub async fn fetch_shared_document(
    keys: &BackupKeys,
    relays: &[String],
    io: &impl RelayIo,
) -> Result<Option<SharedDocument>> {
    Ok(fetch_remote(keys, relays, io).await?.document)
}

/// What the relays hold: the document, and the time of the newest event even when its
/// document is in an older format.
struct Remote {
    document: Option<SharedDocument>,
    created_at: Option<u64>,
}

async fn fetch_remote(keys: &BackupKeys, relays: &[String], io: &impl RelayIo) -> Result<Remote> {
    if relays.is_empty() {
        return Err(Error::NoRelays);
    }
    let Some(event) = io
        .fetch_newest(relays, &keys.pubkey_hex(), SHARED_D_TAG)
        .await?
    else {
        return Ok(Remote {
            document: None,
            created_at: None,
        });
    };
    let document = read_backup_payload(&event, SHARED_D_TAG)
        .and_then(|payload| open_shared_document(payload, &keys.encryption_key))
        .map_err(|_| Error::WrongPassphrase)?;
    Ok(Remote {
        document,
        created_at: Some(event.created_at.as_secs()),
    })
}

/// A vault event is replaceable: of two in the same second, relays keep only one, and not
/// always the newer write. So a new event is always later than the one it was merged with.
fn publish_time(after: Option<u64>) -> u64 {
    let now = (time::now_ms() / 1000) as u64;
    after.map_or(now, |previous| now.max(previous + 1))
}

/// The desktop record on the relays, opened, and the time of its event.
async fn fetch_desktop(
    keys: &BackupKeys,
    relays: &[String],
    io: &impl RelayIo,
) -> Result<(Option<DesktopDocument>, Option<u64>)> {
    if relays.is_empty() {
        return Err(Error::NoRelays);
    }
    let Some(event) = io
        .fetch_newest(relays, &keys.pubkey_hex(), DESKTOP_D_TAG)
        .await?
    else {
        return Ok((None, None));
    };
    let document = read_backup_payload(&event, DESKTOP_D_TAG)
        .and_then(|payload| open_desktop_document(payload, &keys.encryption_key))?;
    Ok((Some(document), Some(event.created_at.as_secs())))
}

async fn publish_desktop(
    document: &DesktopDocument,
    keys: &BackupKeys,
    relays: &[String],
    io: &impl RelayIo,
    previous: Option<u64>,
) -> Result<()> {
    let payload = seal_desktop_document(document, &keys.encryption_key)?;
    let event = build_backup_event(keys, DESKTOP_D_TAG, &payload, publish_time(previous));
    io.publish(relays, &event).await?;
    Ok(())
}

pub async fn publish_shared_document(
    document: &SharedDocument,
    keys: &BackupKeys,
    relays: &[String],
    io: &impl RelayIo,
) -> Result<()> {
    publish_after(document, keys, relays, io, None).await
}

async fn publish_after(
    document: &SharedDocument,
    keys: &BackupKeys,
    relays: &[String],
    io: &impl RelayIo,
    previous: Option<u64>,
) -> Result<()> {
    if relays.is_empty() {
        return Err(Error::NoRelays);
    }
    let payload = seal_shared_document(document, &keys.encryption_key)?;
    let event = build_backup_event(keys, SHARED_D_TAG, &payload, publish_time(previous));
    io.publish(relays, &event).await?;
    Ok(())
}

pub struct SharedRound {
    pub document: SharedDocument,
    pub published: bool,
    /// The merge differs from the document this device held.
    pub changed: bool,
    pub report: ChangeReport,
}

/// Read, merge, and send back when the relay copy differs. Port of `manualSync`.
pub async fn back_up_shared(
    local: &SharedDocument,
    keys: &BackupKeys,
    relays: &[String],
    io: &impl RelayIo,
) -> Result<SharedRound> {
    let fetched = fetch_remote(keys, relays, io).await?;
    let remote = fetched.document;
    let merged = match &remote {
        Some(remote) => merge_shared_documents(local, remote),
        None => local.clone(),
    };
    let merged_canonical = canonical_shared_document(&merged);
    let published = remote
        .as_ref()
        .is_none_or(|remote| merged_canonical != canonical_shared_document(remote));
    let changed = merged_canonical != canonical_shared_document(local);
    if published {
        publish_after(&merged, keys, relays, io, fetched.created_at).await?;
    }
    let report = describe_changes(local, remote.as_ref(), &merged, published);
    Ok(SharedRound {
        document: merged,
        published,
        changed,
        report,
    })
}

/// Runs the flows against one `Gooti` and one way to reach relays.
pub struct BackupRunner<IO: RelayIo> {
    gooti: Arc<Mutex<Gooti>>,
    io: IO,
    /// One backup at a time: a manual one waits for a quiet one and the other way round.
    running: tokio::sync::Mutex<()>,
    /// A quiet backup was asked for while one was running: run once more after it.
    again: AtomicBool,
    quiet_running: AtomicBool,
    /// Backups running or waiting for `running`. `busy` says whether there are any.
    active: Mutex<usize>,
    busy: tokio::sync::watch::Sender<bool>,
}

/// Counts one backup as busy until it is dropped.
struct BusyGuard<'a> {
    active: &'a Mutex<usize>,
    busy: &'a tokio::sync::watch::Sender<bool>,
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        let mut active = self.active.lock().unwrap_or_else(|p| p.into_inner());
        *active -= 1;
        if *active == 0 {
            self.busy.send_replace(false);
        }
    }
}

impl<IO: RelayIo + 'static> BackupRunner<IO> {
    pub fn new(gooti: Arc<Mutex<Gooti>>, io: IO) -> Self {
        BackupRunner {
            gooti,
            io,
            running: tokio::sync::Mutex::new(()),
            again: AtomicBool::new(false),
            quiet_running: AtomicBool::new(false),
            active: Mutex::new(0),
            busy: tokio::sync::watch::channel(false).0,
        }
    }

    /// Whether a backup runs or waits to run: "Back up now", a restore, or an automatic backup.
    pub fn is_busy(&self) -> bool {
        *self.busy.borrow()
    }

    /// Tells about every change of `is_busy`, so a UI can show that a backup runs.
    pub fn busy_updates(&self) -> tokio::sync::watch::Receiver<bool> {
        self.busy.subscribe()
    }

    fn mark_busy(&self) -> BusyGuard<'_> {
        let mut active = self.active.lock().unwrap_or_else(|p| p.into_inner());
        *active += 1;
        if *active == 1 {
            self.busy.send_replace(true);
        }
        BusyGuard {
            active: &self.active,
            busy: &self.busy,
        }
    }

    fn gooti(&self) -> MutexGuard<'_, Gooti> {
        self.gooti
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// "Back up now": read, merge, send if needed, then apply here.
    pub async fn back_up_now(&self) -> Result<BackupOutcome> {
        let _busy = self.mark_busy();
        let _running = self.running.lock().await;
        self.rounds().await
    }

    /// Backups until nothing new is left to send, a few rounds at most. The caller holds `running`.
    /// Each round sends the desktop record before the shared record. A key that leaves the
    /// shared record (Move key to desktop) is then always in the desktop record first; when
    /// that fails, the shared record is not sent and still holds the key.
    async fn rounds(&self) -> Result<BackupOutcome> {
        let mut published = false;
        let mut changed = false;
        let mut report = ChangeReport::default();
        for _ in 0..MAX_ROUNDS {
            published |= self.desktop_round().await?.0;
            let plan = self.gooti().vault_mut().shared_plan()?;
            let result = back_up_shared(&plan.document, &plan.keys, &plan.relays, &self.io).await?;
            published |= result.published;
            changed |= result.changed;
            merge_reports(&mut report, result.report);
            match self
                .gooti()
                .vault_mut()
                .finish_shared(plan.revision, result.document)?
            {
                Applied::Done => break,
                // An edit arrived while the relays were busy, or a Desktop only key came back
                // and was replaced by its pointer. Backup again so that goes out too.
                Applied::Stale | Applied::SendAgain => {}
            }
        }
        let result = if published {
            BackupResult::BackedUp
        } else if changed {
            BackupResult::Updated
        } else {
            BackupResult::Current
        };
        Ok(BackupOutcome { result, report })
    }

    /// Merges the desktop record with the relays: whether it sent one, and whether the shared
    /// record needs a new pointer now. The caller holds `running`.
    async fn desktop_round(&self) -> Result<(bool, bool)> {
        for _ in 0..MAX_ROUNDS {
            let Some(plan) = self.gooti().vault_mut().desktop_plan()? else {
                return Ok((false, false));
            };
            let (remote, created_at) = fetch_desktop(&plan.keys, &plan.relays, &self.io).await?;
            let merged = match &remote {
                Some(remote) => merge_desktop_documents(&plan.document, remote),
                None => plan.document.clone(),
            };
            let send = remote.as_ref() != Some(&merged);
            if send {
                publish_desktop(&merged, &plan.keys, &plan.relays, &self.io, created_at).await?;
            }
            match self
                .gooti()
                .vault_mut()
                .finish_desktop(&plan.document, merged, false)?
            {
                Applied::Stale => {}
                Applied::Done => return Ok((send, false)),
                Applied::SendAgain => return Ok((send, true)),
            }
        }
        Err(Error::Relays {
            message: "This desktop kept changing. Try again.".into(),
            failures: Vec::new(),
        })
    }

    /// "Use an existing backup" while backup is off: merge with the relay copy and keep backing up.
    pub async fn join(&self, phrase: &str, relays: &[String]) -> Result<()> {
        let _busy = self.mark_busy();
        let _running = self.running.lock().await;
        let relays = normalize_relays(relays)?;
        let keys = derive_shared_keys(phrase)?;
        let desktop_keys = derive_desktop_keys(phrase)?;
        for _ in 0..MAX_ROUNDS {
            let (local, revision) = self.gooti().vault().join_plan()?;
            let fetched = fetch_remote(&keys, &relays, &self.io).await?;
            let remote = fetched.document.ok_or(Error::NoRemoteVault)?;
            let merged = merge_shared_documents(&local, &remote);
            if canonical_shared_document(&merged) != canonical_shared_document(&remote) {
                publish_after(&merged, &keys, &relays, &self.io, fetched.created_at).await?;
            }
            // The vault lock must be gone before the next round waits on the relays.
            let applied = self.gooti().vault_mut().finish_join(
                revision,
                &keys,
                &desktop_keys,
                relays.clone(),
                merged,
            )?;
            match applied {
                // The desktop record next: its keys, signer keys and connections come here,
                // and this desktop's own go into it.
                Applied::Done => {
                    if self.desktop_round().await?.1 {
                        self.rounds().await?;
                    }
                    return Ok(());
                }
                Applied::SendAgain => {
                    self.rounds().await?;
                    return Ok(());
                }
                Applied::Stale => {}
            }
        }
        Err(Error::Relays {
            message: "This desktop kept changing. Try again.".into(),
            failures: Vec::new(),
        })
    }

    /// The first screen's "Restore from backup": the relays are read before anything is stored, so
    /// a missing vault leaves this desktop without one.
    pub async fn restore_new_vault(
        &self,
        password: &str,
        phrase: &str,
        relays: &[String],
    ) -> Result<()> {
        let _busy = self.mark_busy();
        let _running = self.running.lock().await;
        crate::error::check_password(password)?;
        let relays = normalize_relays(relays)?;
        let keys = derive_shared_keys(phrase)?;
        let desktop_keys = derive_desktop_keys(phrase)?;
        if self.gooti().vault().status() != crate::vault::VaultStatus::NoVault {
            return Err(Error::VaultExists);
        }
        let fetched = fetch_remote(&keys, &relays, &self.io).await?;
        let remote = fetched.document.ok_or(Error::NoRemoteVault)?;
        // Read before anything is stored too: Desktop only keys, signer keys, connections.
        let (desktop_remote, _) = fetch_desktop(&desktop_keys, &relays, &self.io).await?;
        let merged = merge_shared_documents(&SharedDocument::default(), &remote);
        if canonical_shared_document(&merged) != canonical_shared_document(&remote) {
            publish_after(&merged, &keys, &relays, &self.io, fetched.created_at).await?;
        }
        let mut gooti = self.gooti();
        let vault = gooti.vault_mut();
        vault.create(password)?;
        let revision = vault.revision()?;
        vault.finish_join(revision, &keys, &desktop_keys, relays, merged)?;
        if let Some(desktop_remote) = desktop_remote
            && let Some(plan) = vault.desktop_plan()?
        {
            let merged = merge_desktop_documents(&plan.document, &desktop_remote);
            let unsent = merged != desktop_remote;
            vault.finish_desktop(&plan.document, merged, unsent)?;
        }
        Ok(())
    }

    /// Automatic backup after a change or an unlock. A failure is only reported to `done`;
    /// the next change or unlock tries again. Calls during a run are folded into one more run.
    pub fn kick(self: &Arc<Self>, done: impl Fn(Result<BackupOutcome>) + Send + Sync + 'static) {
        if !self.gooti().vault().auto_backup() {
            return;
        }
        self.again.store(true, Ordering::SeqCst);
        if self.quiet_running.swap(true, Ordering::SeqCst) {
            return;
        }
        let backup_runner = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                while backup_runner.again.swap(false, Ordering::SeqCst) {
                    done(backup_runner.back_up_now().await);
                }
                backup_runner.quiet_running.store(false, Ordering::SeqCst);
                // A kick that came after the last check saw `quiet_running` still set and left
                // the work to this task. Take it back, unless another task already did.
                if !backup_runner.again.load(Ordering::SeqCst)
                    || backup_runner.quiet_running.swap(true, Ordering::SeqCst)
                {
                    break;
                }
            }
        });
    }
}

/// Collects the lines of several rounds without repeats.
fn merge_reports(into: &mut ChangeReport, more: ChangeReport) {
    for line in more.took {
        if !into.took.contains(&line) {
            into.took.push(line);
        }
    }
    for line in more.sent {
        if !into.sent.contains(&line) {
            into.sent.push(line);
        }
    }
}

/// The relays a new backup starts with.
pub fn default_relays() -> Vec<String> {
    super::state::DEFAULT_BACKUP_RELAYS
        .iter()
        .map(|r| r.to_string())
        .collect()
}

/// For the first screen: is this a valid passphrase? Lets the UI check before it asks the relays.
pub fn check_phrase(phrase: &str) -> Result<()> {
    derive_shared_keys(phrase).map(|_| ())
}
