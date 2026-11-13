//! Shared state behind every command, the tray and the lock watchers.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use gooti_core::backup::flow::BackupRunner;
use gooti_core::backup::relay::WebSocketRelays;
use gooti_core::bunker::service::{BunkerService, Notice};
use gooti_core::{Gooti, VaultStatus};
use serde_json::json;
use tauri::{AppHandle, Emitter, Runtime};

/// Event the UI listens to. Payload: the new `VaultStatus`.
pub const VAULT_STATUS_EVENT: &str = "vault-status";
/// The identities or the backup settings changed outside the UI, such as by an automatic backup.
pub const VAULT_CHANGED_EVENT: &str = "vault-changed";
/// An automatic backup finished. Payload: `{ result, took, sent }` or `{ error }`.
pub const BACKUP_ACTIVITY_EVENT: &str = "backup-activity";
/// A backup started or every backup ended. Payload: `true` while any backup runs.
pub const BACKUP_BUSY_EVENT: &str = "backup-busy";
/// The list of bunker requests waiting for Approve or Reject changed.
pub const BUNKER_PENDING_EVENT: &str = "bunker-pending";
/// An app connected, logged out, or was removed.
pub const BUNKER_CONNECTIONS_EVENT: &str = "bunker-connections";
/// Something new in Activity.
pub const ACTIVITY_EVENT: &str = "activity-changed";
/// A bunker relay connected, was lost, was added, or was dropped. Read `bunker_info`.
pub const BUNKER_RELAYS_EVENT: &str = "bunker-relays";

#[derive(Clone)]
pub struct AppState {
    gooti: Arc<Mutex<Gooti>>,
    backup_runner: Arc<BackupRunner<WebSocketRelays>>,
    /// Runs while the vault is unlocked.
    bunker: Arc<Mutex<Option<Arc<BunkerService>>>>,
    /// Whether this system shows a tray icon. Without one, closing the window quits.
    has_tray: Arc<std::sync::atomic::AtomicBool>,
    /// The update found by the last check, ready to install.
    update: Arc<Mutex<Option<tauri_plugin_updater::Update>>>,
    last_activity: Arc<Mutex<Instant>>,
    /// When the last automatic backup started. The regular check counts from here.
    last_backup: Arc<Mutex<Instant>>,
    /// The running backup is a regular check: the UI shows no progress bar for it.
    quiet_check: Arc<AtomicBool>,
    /// The last backup failed. Activity gets only the first failure in a row.
    backup_failing: Arc<AtomicBool>,
    /// A clicked `nostrconnect://` link, until the unlocked UI takes it.
    connect_link: Arc<Mutex<Option<String>>>,
}

impl AppState {
    pub fn new(gooti: Gooti, relays: WebSocketRelays) -> Self {
        let gooti = Arc::new(Mutex::new(gooti));
        AppState {
            backup_runner: Arc::new(BackupRunner::new(Arc::clone(&gooti), relays)),
            gooti,
            bunker: Arc::new(Mutex::new(None)),
            has_tray: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            update: Arc::new(Mutex::new(None)),
            last_activity: Arc::new(Mutex::new(Instant::now())),
            last_backup: Arc::new(Mutex::new(Instant::now())),
            quiet_check: Arc::new(AtomicBool::new(false)),
            backup_failing: Arc::new(AtomicBool::new(false)),
            connect_link: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_has_tray(&self, has: bool) {
        self.has_tray
            .store(has, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn has_tray(&self) -> bool {
        self.has_tray.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Closing the main window hides it when Gooti should keep running and a tray can bring it back.
    pub fn hide_on_close(&self) -> bool {
        self.has_tray() && self.gooti().settings().keep_running
    }

    pub fn set_update(&self, update: Option<tauri_plugin_updater::Update>) {
        *self.update.lock().unwrap_or_else(|p| p.into_inner()) = update;
    }

    pub fn take_update(&self) -> Option<tauri_plugin_updater::Update> {
        self.update.lock().unwrap_or_else(|p| p.into_inner()).take()
    }

    /// Everything that starts once the vault is open: automatic backup and the bunker.
    pub fn on_unlocked<R: Runtime>(&self, app: &AppHandle<R>) {
        self.kick_backup(app);
        self.start_bunker(app);
    }

    pub fn bunker(&self) -> Option<Arc<BunkerService>> {
        self.bunker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Starts the bunker, or makes a running one re-read its links, apps and relays.
    pub fn start_bunker<R: Runtime>(&self, app: &AppHandle<R>) {
        let mut slot = self.bunker.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(service) = slot.as_ref() {
            in_runtime(|| service.refresh());
            return;
        }
        let app = app.clone();
        let state = self.clone();
        let notify = Arc::new(move |notice: Notice| match notice {
            Notice::Asked { ask } => {
                let name = state.gooti().vault().bunker_app_name(&ask.connection_id);
                crate::approval::notify(&app, &name, &ask);
                crate::approval::open(&app);
                let _ = app.emit(BUNKER_PENDING_EVENT, ());
            }
            Notice::Pending => {
                let _ = app.emit(BUNKER_PENDING_EVENT, ());
                if state.bunker().is_none_or(|b| b.pending().is_empty()) {
                    crate::approval::close(&app);
                }
            }
            Notice::Connections => {
                let _ = app.emit(BUNKER_CONNECTIONS_EVENT, ());
            }
            Notice::Activity => {
                let _ = app.emit(ACTIVITY_EVENT, ());
            }
            Notice::Relays => {
                let _ = app.emit(BUNKER_RELAYS_EVENT, ());
            }
        });
        *slot = Some(in_runtime(|| {
            BunkerService::start(Arc::clone(&self.gooti), notify)
        }));
    }

    /// After a change that can affect which identities or relays the bunker serves.
    pub fn refresh_bunker(&self) {
        if let Some(service) = self.bunker() {
            in_runtime(|| service.refresh());
        }
    }

    fn stop_bunker(&self) {
        if let Some(service) = self.bunker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            in_runtime(|| service.stop());
        }
    }

    pub fn backup_runner(&self) -> &BackupRunner<WebSocketRelays> {
        &self.backup_runner
    }

    /// Tells the UI when backups start and end, for the progress bar. Runs for the app's lifetime.
    /// A regular check runs without the bar.
    pub fn watch_backup<R: Runtime>(&self, app: &AppHandle<R>) {
        let mut updates = self.backup_runner.busy_updates();
        let state = self.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while updates.changed().await.is_ok() {
                let busy = *updates.borrow_and_update();
                if !(busy && state.quiet_check.load(Ordering::SeqCst)) {
                    let _ = app.emit(BACKUP_BUSY_EVENT, busy);
                }
            }
        });
    }

    pub fn set_connect_link(&self, link: String) {
        *self.connect_link.lock().unwrap_or_else(|p| p.into_inner()) = Some(link);
    }

    /// The waiting link, once. While locked it keeps waiting for the unlock.
    pub fn take_connect_link(&self) -> Option<String> {
        if !self.gooti().vault().is_unlocked() {
            return None;
        }
        self.connect_link
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
    }

    /// Whether the UI should show a backup running: not for a regular check.
    pub fn backup_busy(&self) -> bool {
        self.backup_runner.is_busy() && !self.quiet_check.load(Ordering::SeqCst)
    }

    /// How long since the last automatic backup started.
    pub fn since_last_backup(&self) -> Duration {
        self.last_backup
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .elapsed()
    }

    /// Starts an automatic backup when it is on. Its result goes to the UI as events.
    pub fn kick_backup<R: Runtime>(&self, app: &AppHandle<R>) {
        self.start_backup(app, false);
    }

    /// The regular check (`backup_timer`): a normal automatic backup without the progress bar.
    /// Skipped while another backup runs.
    pub fn check_backup<R: Runtime>(&self, app: &AppHandle<R>) {
        if !self.backup_runner.is_busy() {
            self.start_backup(app, true);
        }
    }

    fn start_backup<R: Runtime>(&self, app: &AppHandle<R>, quiet: bool) {
        *self.last_backup.lock().unwrap_or_else(|p| p.into_inner()) = Instant::now();
        let was_quiet = self.quiet_check.swap(quiet, Ordering::SeqCst);
        if was_quiet && !quiet && self.backup_runner.is_busy() {
            // A change arrived during a quiet check: from now on it is a backup the user should see.
            let _ = app.emit(BACKUP_BUSY_EVENT, true);
        }
        let backup_runner = Arc::clone(&self.backup_runner);
        let state = self.clone();
        let app = app.clone();
        // `kick` spawns on tokio, so it must start from inside the runtime.
        tauri::async_runtime::spawn(async move {
            backup_runner.kick(move |outcome| {
                state.record_backup(&app, true, &outcome);
                let payload = match outcome {
                    Ok(outcome) => {
                        // A backup can delete identities, and with them their bunker apps.
                        state.refresh_bunker();
                        let _ = app.emit(VAULT_CHANGED_EVENT, ());
                        json!(outcome)
                    }
                    Err(error) => json!({ "error": error.to_string() }),
                };
                let _ = app.emit(BACKUP_ACTIVITY_EVENT, payload);
            });
        });
    }

    pub fn gooti(&self) -> MutexGuard<'_, Gooti> {
        // A panic while holding the lock must not take the whole app down with it.
        self.gooti
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn touch(&self) {
        *self
            .last_activity
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Instant::now();
    }

    pub fn idle_for(&self) -> std::time::Duration {
        self.last_activity
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .elapsed()
    }

    /// Locks when unlocked and tells the UI. Returns whether it locked.
    pub fn lock<R: Runtime>(&self, app: &AppHandle<R>) -> bool {
        self.stop_bunker();
        crate::approval::close(app);
        let locked = {
            let mut gooti = self.gooti();
            let was_unlocked = gooti.vault().is_unlocked();
            gooti.vault_mut().lock();
            was_unlocked
        };
        if locked {
            emit_status(app, VaultStatus::Locked);
        }
        locked
    }

    /// Puts a backup's result in Activity. "Up to date" from an automatic backup is not worth a row.
    pub fn record_backup<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        automatic: bool,
        outcome: &gooti_core::Result<gooti_core::backup::flow::BackupOutcome>,
    ) {
        use gooti_core::activity::{Activity, ActivityEntry};
        use gooti_core::backup::flow::BackupResult;
        let failed_before = self.backup_failing.swap(outcome.is_err(), Ordering::SeqCst);
        let what = match outcome {
            Ok(outcome) if automatic && outcome.result == BackupResult::Current => return,
            // The relays are still out of reach. One row says so; the next success shows again.
            Err(_) if automatic && failed_before => return,
            Ok(outcome) => Activity::Backup {
                automatic,
                result: serde_json::to_value(outcome.result)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_default(),
                took: outcome.report.took.clone(),
                sent: outcome.report.sent.clone(),
                error: None,
            },
            Err(error) => Activity::Backup {
                automatic,
                result: "failed".into(),
                took: Vec::new(),
                sent: Vec::new(),
                error: Some(error.to_string()),
            },
        };
        let mut gooti = self.gooti();
        gooti.vault_mut().record_activity(ActivityEntry {
            at: gooti_core::time_now_ms(),
            what,
        });
        let _ = gooti.vault_mut().flush_activity();
        drop(gooti);
        let _ = app.emit(ACTIVITY_EVENT, ());
    }

    /// Pauses or resumes every app. Returns the new state, or `None` while locked.
    pub fn toggle_pause<R: Runtime>(&self, app: &AppHandle<R>) -> Option<bool> {
        let paused = {
            let mut gooti = self.gooti();
            let paused = !gooti.vault().bunker_info().ok()?.paused;
            gooti.vault_mut().bunker_set_paused(paused).ok()?;
            paused
        };
        self.refresh_bunker();
        let _ = app.emit(BUNKER_CONNECTIONS_EVENT, ());
        Some(paused)
    }

    /// Deletes everything; the bunker stops first.
    pub fn reset(&self) -> gooti_core::Result<()> {
        self.stop_bunker();
        self.gooti().reset()
    }
}

pub fn emit_status<R: Runtime>(app: &AppHandle<R>, status: VaultStatus) {
    let _ = app.emit(VAULT_STATUS_EVENT, status);
}

/// Runs `work` inside Tauri's tokio runtime, so it may spawn tasks. Commands that are not
/// `async` run on the main thread, outside of it.
fn in_runtime<T>(work: impl FnOnce() -> T) -> T {
    let handle = tauri::async_runtime::handle();
    let _guard = handle.inner().enter();
    work()
}

#[cfg(test)]
mod tests {
    use gooti_core::activity::Activity;
    use gooti_core::backup::flow::{BackupOutcome, BackupResult};
    use gooti_core::backup::relay::WebSocketRelays;
    use gooti_core::{Error, Gooti, KdfParams, MemoryKeychain};
    use tauri::test::{mock_builder, mock_context, noop_assets};

    use super::AppState;

    fn outcome(result: BackupResult) -> gooti_core::Result<BackupOutcome> {
        Ok(BackupOutcome {
            result,
            report: Default::default(),
        })
    }

    #[test]
    fn only_the_first_automatic_failure_in_a_row_reaches_activity() {
        let dir = tempfile::tempdir().unwrap();
        let mut gooti = Gooti::open_with_kdf(
            dir.path(),
            Box::new(MemoryKeychain::default()),
            KdfParams::TEST,
        );
        gooti.vault_mut().create("correct horse battery").unwrap();
        let state = AppState::new(gooti, WebSocketRelays::default());
        let app = mock_builder().build(mock_context(noop_assets())).unwrap();
        let app = app.handle();
        let failures = || {
            state
                .gooti()
                .vault()
                .activity()
                .unwrap()
                .iter()
                .filter(|entry| matches!(&entry.what, Activity::Backup { error: Some(_), .. }))
                .count()
        };

        state.record_backup(app, true, &Err(Error::NoRemoteVault));
        state.record_backup(app, true, &Err(Error::NoRemoteVault));
        state.record_backup(app, true, &Err(Error::NoRemoteVault));
        assert_eq!(failures(), 1, "relays still out of reach: one row");

        // A quiet "up to date" adds no row, but the relays work again.
        state.record_backup(app, true, &outcome(BackupResult::Current));
        state.record_backup(app, true, &Err(Error::NoRemoteVault));
        assert_eq!(failures(), 2);

        // "Back up now" always shows its failure.
        state.record_backup(app, false, &Err(Error::NoRemoteVault));
        assert_eq!(failures(), 3);
    }
}
