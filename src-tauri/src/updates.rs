//! New versions. Gooti installs an update itself when it runs as an AppImage or on Windows (the
//! updater runs the new installer). A deb, rpm or AUR install is updated by its package manager;
//! Gooti only says that a version is out.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::state::AppState;

/// A new version was found. Payload: `UpdateInfo`.
pub const UPDATE_EVENT: &str = "update-available";

/// Release page for people who update through a package manager.
pub const RELEASES_URL: &str = "https://github.com/sam-hayes-org/gooti-desktop/releases/latest";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
    /// Gooti can install it itself (AppImage, Windows). Otherwise use the package manager.
    pub can_install: bool,
    pub releases_url: &'static str,
}

/// Gooti can replace itself: on Windows (installed by its installer), or as an AppImage (the
/// AppImage runtime sets `APPIMAGE` to the file's path).
pub fn can_install() -> bool {
    cfg!(windows) || std::env::var_os("APPIMAGE").is_some()
}

/// Asks the release endpoint for a newer version. `GOOTI_UPDATE_URL` replaces the endpoint for
/// testing; the update must still carry a signature from the release key either way.
pub async fn check<R: Runtime>(app: &AppHandle<R>) -> Result<Option<UpdateInfo>, String> {
    let mut builder = app.updater_builder();
    if let Ok(url) = std::env::var("GOOTI_UPDATE_URL") {
        let url = url
            .parse()
            .map_err(|_| "GOOTI_UPDATE_URL is not a URL".to_string())?;
        builder = builder.endpoints(vec![url]).map_err(|e| e.to_string())?;
    }
    let updater = builder.build().map_err(|e| e.to_string())?;
    let found = updater.check().await.map_err(|e| e.to_string())?;
    let info = found.as_ref().map(info_for);
    if let Some(state) = app.try_state::<AppState>() {
        state.set_update(found);
    }
    Ok(info)
}

/// Downloads, checks the signature, replaces the AppImage, and restarts. On Windows the updater
/// starts the installer and quits Gooti; the installer starts it again.
pub async fn install<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if !can_install() {
        return Err("Update Gooti with your package manager.".into());
    }
    let update = app
        .try_state::<AppState>()
        .and_then(|state| state.take_update())
        .ok_or("Look for updates first.")?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| format!("The update could not be installed: {e}"))?;
    app.restart()
}

/// At start, when "Check for updates" is on. A failure is silent: the next start tries again.
pub fn check_at_start<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Ok(Some(info)) = check(&app).await {
            let _ = app.emit(UPDATE_EVENT, info);
        }
    });
}

fn info_for(update: &Update) -> UpdateInfo {
    UpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone(),
        can_install: can_install(),
        releases_url: RELEASES_URL,
    }
}
