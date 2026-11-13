//! Commands the UI may call. Each one is a thin wrapper around `gooti-core`.
//! Errors reach the UI as `{ code, message, failures? }`. The message is shown, the code is for logic.

use crate::updates::UpdateInfo;
use gooti_core::activity::ActivityEntry;
use gooti_core::backup::flow::{self, BackupOutcome};
use gooti_core::backup::phrase::generate_passphrase;
use gooti_core::bunker::handler::Ask;
use gooti_core::bunker::policy::{Remember, Trust};
use gooti_core::bunker::pool::RelayStatus;
use gooti_core::bunker::store::{BunkerInfo, ConnectionDetail};
use gooti_core::{
    AppSettingsChange, BackupInfo, Custody, IdentitySummary, LockSettingsChange, NewKey, Settings,
    VaultStatus,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::state::{AppState, VAULT_CHANGED_EVENT, emit_status};

#[derive(Debug, Serialize)]
pub struct CommandError {
    code: &'static str,
    message: String,
    /// Each relay's own reason, when every relay failed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    failures: Vec<gooti_core::RelayFailure>,
}

impl From<gooti_core::Error> for CommandError {
    fn from(error: gooti_core::Error) -> Self {
        CommandError {
            code: error.code(),
            message: error.to_string(),
            failures: error.relay_failures().to_vec(),
        }
    }
}

type CommandResult<T> = Result<T, CommandError>;

/// Runs slow work (Argon2id, scrypt) off the async workers.
async fn blocking<T: Send + 'static>(
    state: &AppState,
    work: impl FnOnce(&AppState) -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    let state = state.clone();
    tauri::async_runtime::spawn_blocking(move || work(&state))
        .await
        .unwrap_or_else(|_| {
            Err(CommandError {
                code: "internal",
                message: "Something went wrong. Try again.".into(),
                failures: Vec::new(),
            })
        })
}

#[tauri::command]
pub fn app_info() -> gooti_core::AppInfo {
    gooti_core::app_info()
}

#[tauri::command]
pub fn activity_touch(state: State<'_, AppState>) {
    state.touch();
}

// Vault

#[tauri::command]
pub fn vault_status(state: State<'_, AppState>) -> VaultStatus {
    state.gooti().vault().status()
}

#[tauri::command]
pub async fn vault_create<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    password: String,
) -> CommandResult<()> {
    blocking(&state, move |state| {
        state.gooti().vault_mut().create(&password)?;
        Ok(())
    })
    .await?;
    state.touch();
    emit_status(&app, VaultStatus::Unlocked);
    state.on_unlocked(&app);
    Ok(())
}

#[tauri::command]
pub async fn vault_unlock<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    password: String,
) -> CommandResult<()> {
    blocking(&state, move |state| {
        state.gooti().vault_mut().unlock(&password)?;
        Ok(())
    })
    .await?;
    state.touch();
    emit_status(&app, VaultStatus::Unlocked);
    state.on_unlocked(&app);
    Ok(())
}

#[tauri::command]
pub fn vault_lock<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) {
    state.lock(&app);
}

#[tauri::command]
pub fn vault_reset<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CommandResult<()> {
    state.reset()?;
    emit_status(&app, VaultStatus::NoVault);
    Ok(())
}

#[tauri::command]
pub async fn vault_change_password(
    state: State<'_, AppState>,
    current: String,
    next: String,
) -> CommandResult<()> {
    state.touch();
    blocking(&state, move |state| {
        state.gooti().vault_mut().change_password(&current, &next)?;
        Ok(())
    })
    .await
}

// Identities

#[tauri::command]
pub fn identities_list(state: State<'_, AppState>) -> CommandResult<Vec<IdentitySummary>> {
    Ok(state.gooti().vault().identities()?)
}

#[tauri::command]
pub fn identity_get(state: State<'_, AppState>, id: String) -> CommandResult<IdentitySummary> {
    Ok(state.gooti().vault().identity(&id)?)
}

/// Without `key`, a new private key is generated in the core and never shown.
#[tauri::command]
pub async fn identity_create<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    nick: String,
    key: Option<String>,
    key_password: Option<String>,
    custody: Option<Custody>,
    relays: Option<Vec<String>>,
) -> CommandResult<IdentitySummary> {
    state.touch();
    let custody = custody.unwrap_or(Custody::Shared);
    let created = blocking(&state, move |state| {
        let new_key = match key.as_deref() {
            Some(key) => NewKey::Import {
                key,
                password: key_password.as_deref(),
            },
            None => NewKey::Generate,
        };
        Ok(state
            .gooti()
            .vault_mut()
            .add_identity_on(&nick, new_key, custody, relays.as_deref())?)
    })
    .await;
    if created.is_ok() {
        state.refresh_bunker();
        state.kick_backup(&app);
    }
    created
}

/// "Move key to desktop" (`desktop`) or "Use in browsers again" (`shared`).
#[tauri::command]
pub fn identity_set_custody<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    id: String,
    custody: Custody,
    relays: Option<Vec<String>>,
) -> CommandResult<IdentitySummary> {
    state.touch();
    let updated = state
        .gooti()
        .vault_mut()
        .set_custody_on(&id, custody, relays.as_deref())?;
    state.refresh_bunker();
    state.kick_backup(&app);
    let _ = app.emit(crate::state::BUNKER_CONNECTIONS_EVENT, ());
    Ok(updated)
}

/// "Sign here instead": this desktop answers for these identities. The desktop that did so
/// before stops at its next backup.
#[tauri::command]
pub fn identity_serve_here<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> CommandResult<Vec<IdentitySummary>> {
    state.touch();
    let identities = {
        let mut gooti = state.gooti();
        gooti.vault_mut().serve_here(&ids)?;
        gooti.vault().identities()?
    };
    state.refresh_bunker();
    state.kick_backup(&app);
    Ok(identities)
}

#[tauri::command]
pub fn identity_rename<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    id: String,
    nick: String,
) -> CommandResult<IdentitySummary> {
    state.touch();
    let renamed = state.gooti().vault_mut().rename_identity(&id, &nick)?;
    state.kick_backup(&app);
    Ok(renamed)
}

#[tauri::command]
pub fn identity_set_color<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    id: String,
    color: Option<String>,
) -> CommandResult<IdentitySummary> {
    state.touch();
    let updated = state
        .gooti()
        .vault_mut()
        .set_identity_color(&id, color.as_deref())?;
    state.kick_backup(&app);
    Ok(updated)
}

#[tauri::command]
pub fn identity_delete<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    state.touch();
    state.gooti().vault_mut().delete_identity(&id)?;
    state.refresh_bunker();
    state.kick_backup(&app);
    Ok(())
}

/// The one place a private key goes to the UI: the user chose "Show".
#[tauri::command]
pub fn identity_reveal_nsec(state: State<'_, AppState>, id: String) -> CommandResult<String> {
    state.touch();
    Ok(state.gooti().vault().reveal_nsec(&id)?.to_string())
}

#[tauri::command]
pub async fn identity_export_ncryptsec(
    state: State<'_, AppState>,
    id: String,
    password: String,
) -> CommandResult<String> {
    state.touch();
    blocking(&state, move |state| {
        Ok(state.gooti().vault().export_ncryptsec(&id, &password)?)
    })
    .await
}

// Settings

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> Settings {
    state.gooti().settings().clone()
}

#[tauri::command]
pub fn settings_set_unlock_at_login(
    state: State<'_, AppState>,
    on: bool,
) -> CommandResult<Settings> {
    let mut gooti = state.gooti();
    gooti.set_unlock_at_login(on)?;
    Ok(gooti.settings().clone())
}

#[tauri::command]
pub fn settings_change_lock(
    state: State<'_, AppState>,
    change: LockSettingsChange,
) -> CommandResult<Settings> {
    Ok(state.gooti().change_lock_settings(change)?)
}

// Backup

/// Whether a backup runs right now. Later changes arrive as `backup-busy` events.
#[tauri::command]
pub fn backup_busy(state: State<'_, AppState>) -> bool {
    state.backup_busy()
}

/// `null` while backup is off.
#[tauri::command]
pub fn backup_info(state: State<'_, AppState>) -> CommandResult<Option<BackupInfo>> {
    Ok(state.gooti().vault().backup_info()?)
}

#[tauri::command]
pub fn backup_default_relays() -> Vec<String> {
    flow::default_relays()
}

/// 12 new words. Shown once; the vault stores only the keys made from them.
#[tauri::command]
pub fn backup_generate_passphrase() -> String {
    generate_passphrase().to_string()
}

#[tauri::command]
pub fn backup_check_passphrase(phrase: String) -> CommandResult<()> {
    Ok(flow::check_phrase(&phrase)?)
}

#[tauri::command]
pub fn backup_enable(
    state: State<'_, AppState>,
    phrase: String,
    relays: Vec<String>,
) -> CommandResult<Option<BackupInfo>> {
    state.touch();
    let mut gooti = state.gooti();
    gooti.vault_mut().enable_backup(&phrase, &relays)?;
    Ok(gooti.vault().backup_info()?)
}

/// "Use an existing backup": merge this vault with the one on the relays.
#[tauri::command]
pub async fn backup_join<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    phrase: String,
    relays: Vec<String>,
) -> CommandResult<Option<BackupInfo>> {
    state.touch();
    state.backup_runner().join(&phrase, &relays).await?;
    state.refresh_bunker();
    let _ = app.emit(VAULT_CHANGED_EVENT, ());
    Ok(state.gooti().vault().backup_info()?)
}

#[tauri::command]
pub async fn backup_now<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> CommandResult<BackupOutcome> {
    state.touch();
    let outcome = state.backup_runner().back_up_now().await;
    state.record_backup(&app, false, &outcome);
    let outcome = outcome?;
    state.refresh_bunker();
    let _ = app.emit(VAULT_CHANGED_EVENT, ());
    Ok(outcome)
}

#[tauri::command]
pub fn backup_set_relays(
    state: State<'_, AppState>,
    relays: Vec<String>,
) -> CommandResult<Option<BackupInfo>> {
    let mut gooti = state.gooti();
    gooti.vault_mut().set_backup_relays(&relays)?;
    Ok(gooti.vault().backup_info()?)
}

/// Turning it on backups once.
#[tauri::command]
pub fn backup_set_auto<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    on: bool,
) -> CommandResult<Option<BackupInfo>> {
    let info = {
        let mut gooti = state.gooti();
        gooti.vault_mut().set_auto_backup(on)?;
        gooti.vault().backup_info()?
    };
    if on {
        state.kick_backup(&app);
    }
    Ok(info)
}

#[tauri::command]
pub fn backup_disable(state: State<'_, AppState>) -> CommandResult<()> {
    Ok(state.gooti().vault_mut().disable_backup()?)
}

/// The first screen's "Restore from backup". Nothing is stored unless the relays have a vault.
#[tauri::command]
pub async fn vault_restore<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    password: String,
    phrase: String,
    relays: Vec<String>,
) -> CommandResult<()> {
    state
        .backup_runner()
        .restore_new_vault(&password, &phrase, &relays)
        .await?;
    state.touch();
    emit_status(&app, VaultStatus::Unlocked);
    state.on_unlocked(&app);
    Ok(())
}

// Bunker

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BunkerView {
    #[serde(flatten)]
    info: BunkerInfo,
    /// Each relay the bunker holds right now, and whether it is connected.
    relay_status: Vec<RelayStatus>,
}

#[tauri::command]
pub fn bunker_info(state: State<'_, AppState>) -> CommandResult<BunkerView> {
    let info = state.gooti().vault().bunker_info()?;
    let relay_status = state.bunker().map(|b| b.relay_status()).unwrap_or_default();
    Ok(BunkerView { info, relay_status })
}

#[tauri::command]
pub fn bunker_pending(state: State<'_, AppState>) -> Vec<Ask> {
    state.bunker().map(|b| b.pending()).unwrap_or_default()
}

/// A new single-use `bunker://` link for this identity, valid for an hour, on these relays
/// (`null`: the default relays for apps).
#[tauri::command]
pub fn bunker_link(
    state: State<'_, AppState>,
    identity_id: String,
    relays: Option<Vec<String>>,
) -> CommandResult<String> {
    state.touch();
    let link = state
        .gooti()
        .vault_mut()
        .bunker_link_on(&identity_id, relays.as_deref())?;
    state.refresh_bunker();
    Ok(link)
}

/// A clicked `nostrconnect://` link waiting for "Connect new app", once; `null` while locked.
#[tauri::command]
pub fn connect_link_take(state: State<'_, AppState>) -> Option<String> {
    state.take_connect_link()
}

/// Connects the app behind a pasted `nostrconnect://` link.
#[tauri::command]
pub fn bunker_connect(
    state: State<'_, AppState>,
    link: String,
    identity_id: String,
) -> CommandResult<()> {
    state.touch();
    let bunker = state.bunker().ok_or(gooti_core::Error::Locked)?;
    Ok(bunker.accept_nostrconnect(&link, &identity_id)?)
}

/// Approve or Reject, and how long to remember it: `once`, `fiveMinutes`, `oneHour`,
/// `oneDay` or `always`.
#[tauri::command]
pub fn bunker_decide(
    state: State<'_, AppState>,
    key: String,
    approve: bool,
    remember: Remember,
) -> CommandResult<bool> {
    state.touch();
    let bunker = state.bunker().ok_or(gooti_core::Error::Locked)?;
    Ok(bunker.decide(&key, approve, remember)?)
}

#[tauri::command]
pub fn bunker_connection(
    state: State<'_, AppState>,
    connection_id: String,
) -> CommandResult<ConnectionDetail> {
    Ok(state.gooti().vault().bunker_connection(&connection_id)?)
}

#[tauri::command]
pub fn bunker_set_trust(
    state: State<'_, AppState>,
    connection_id: String,
    trust: Trust,
) -> CommandResult<ConnectionDetail> {
    let mut gooti = state.gooti();
    gooti.vault_mut().bunker_set_trust(&connection_id, trust)?;
    Ok(gooti.vault().bunker_connection(&connection_id)?)
}

/// Removes a remembered choice.
#[tauri::command]
pub fn bunker_forget(
    state: State<'_, AppState>,
    connection_id: String,
    method: String,
    kind: Option<u16>,
) -> CommandResult<ConnectionDetail> {
    let mut gooti = state.gooti();
    gooti
        .vault_mut()
        .bunker_forget(&connection_id, &method, kind)?;
    Ok(gooti.vault().bunker_connection(&connection_id)?)
}

// Activity

/// Newest first.
#[tauri::command]
pub fn activity_list(state: State<'_, AppState>) -> CommandResult<Vec<ActivityEntry>> {
    Ok(state.gooti().vault().activity()?)
}

#[tauri::command]
pub fn activity_clear(state: State<'_, AppState>) -> CommandResult<()> {
    Ok(state.gooti().vault_mut().clear_activity()?)
}

#[tauri::command]
pub fn bunker_revoke(state: State<'_, AppState>, connection_id: String) -> CommandResult<()> {
    state.gooti().vault_mut().bunker_revoke(&connection_id)?;
    state.refresh_bunker();
    Ok(())
}

#[tauri::command]
pub fn bunker_set_paused(state: State<'_, AppState>, paused: bool) -> CommandResult<()> {
    state.gooti().vault_mut().bunker_set_paused(paused)?;
    state.refresh_bunker();
    Ok(())
}

/// The default relays for apps (Settings). Changes nothing in use.
#[tauri::command]
pub fn bunker_set_relays(
    state: State<'_, AppState>,
    relays: Vec<String>,
) -> CommandResult<Vec<String>> {
    let relays = state.gooti().vault_mut().bunker_set_relays(&relays)?;
    state.refresh_bunker();
    Ok(relays)
}

/// The relays Gooti browsers reach a Desktop only identity on. Backup takes them to the browsers.
#[tauri::command]
pub fn bunker_set_app_relays<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    connection_id: String,
    relays: Vec<String>,
) -> CommandResult<Vec<String>> {
    state.touch();
    let relays = state
        .gooti()
        .vault_mut()
        .bunker_set_app_relays(&connection_id, &relays)?;
    state.refresh_bunker();
    state.kick_backup(&app);
    Ok(relays)
}

// App

#[tauri::command]
pub fn settings_change_app(
    state: State<'_, AppState>,
    change: AppSettingsChange,
) -> CommandResult<Settings> {
    Ok(state.gooti().change_app_settings(change)?)
}

/// What the UI needs for the app settings: whether there is a tray to keep running in.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutostartView {
    enabled: bool,
    has_tray: bool,
}

/// "Start Gooti when you log in": the system's login item is the record, not Gooti's settings.
#[tauri::command]
pub fn autostart_get<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> AutostartView {
    use tauri_plugin_autostart::ManagerExt;
    AutostartView {
        enabled: app.autolaunch().is_enabled().unwrap_or(false),
        has_tray: state.has_tray(),
    }
}

#[tauri::command]
pub fn autostart_set<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    on: bool,
) -> CommandResult<AutostartView> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    let result = if on {
        launcher.enable()
    } else {
        launcher.disable()
    };
    result.map_err(|error| CommandError {
        code: "autostart",
        message: format!("Could not change the login item: {error}"),
        failures: Vec::new(),
    })?;
    Ok(autostart_get(app, state))
}

/// `null` when this is the newest version.
#[tauri::command]
pub async fn update_check<R: Runtime>(app: AppHandle<R>) -> CommandResult<Option<UpdateInfo>> {
    crate::updates::check(&app)
        .await
        .map_err(|message| CommandError {
            code: "update",
            message: format!("Could not look for updates: {message}"),
            failures: Vec::new(),
        })
}

/// Installs the update the last check found, then restarts. AppImage only.
#[tauri::command]
pub async fn update_install<R: Runtime>(app: AppHandle<R>) -> CommandResult<()> {
    crate::updates::install(&app)
        .await
        .map_err(|message| CommandError {
            code: "update",
            message,
            failures: Vec::new(),
        })
}

/// The Info page's links. The UI names one; it cannot open any other address.
fn info_link_url(link: &str) -> Option<&'static str> {
    match link {
        "website" => Some("https://getgooti.com"),
        "source" => Some("https://github.com/sam-hayes-org/gooti-desktop"),
        "author" => Some("https://sam-hayes.org"),
        _ => None,
    }
}

/// Opens one of the Info page's links in the browser.
#[tauri::command]
pub fn open_link<R: Runtime>(app: AppHandle<R>, link: String) -> CommandResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let url = info_link_url(&link).ok_or_else(|| CommandError {
        code: "open",
        message: "Gooti does not know this link.".into(),
        failures: Vec::new(),
    })?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| CommandError {
            code: "open",
            message: format!("Could not open the browser: {error}"),
            failures: Vec::new(),
        })
}

/// Opens the release page in the browser, for installs that update through a package manager.
#[tauri::command]
pub fn open_releases<R: Runtime>(app: AppHandle<R>) -> CommandResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(crate::updates::RELEASES_URL, None::<&str>)
        .map_err(|error| CommandError {
            code: "open",
            message: format!("Could not open the browser: {error}"),
            failures: Vec::new(),
        })
}

/// Info → Open-source licenses: Gooti's license and those of everything it ships with
/// (`tools/licenses/generate.sh`). Every bundle carries the file; a debug build reads the repo's.
#[tauri::command]
pub fn licenses_text<R: Runtime>(app: AppHandle<R>) -> CommandResult<String> {
    const FILE: &str = "licenses/THIRD-PARTY-NOTICES.txt";
    let bundled = app
        .path()
        .resolve(FILE, tauri::path::BaseDirectory::Resource)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok());
    #[cfg(debug_assertions)]
    let bundled = bundled.or_else(|| {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FILE)).ok()
    });
    bundled.ok_or_else(|| CommandError {
        code: "licenses",
        message: "The license list is missing from this installation.".into(),
        failures: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::info_link_url;

    #[test]
    fn info_links_open_only_fixed_addresses() {
        assert_eq!(info_link_url("website"), Some("https://getgooti.com"));
        assert_eq!(
            info_link_url("source"),
            Some("https://github.com/sam-hayes-org/gooti-desktop")
        );
        assert_eq!(info_link_url("author"), Some("https://sam-hayes.org"));
        assert_eq!(info_link_url("https://example.com"), None);
    }
}
