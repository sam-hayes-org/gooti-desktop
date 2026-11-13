//! Tauri shell. Windows, tray and commands only; the logic lives in `gooti-core`.

mod approval;
mod autolock;
mod backup_timer;
mod commands;
mod keychain;
mod links;
mod state;
mod tray;
mod updates;

use gooti_core::Gooti;
use gooti_core::backup::relay::WebSocketRelays;
use tauri::{Manager, Runtime};

use crate::keychain::OsKeychain;
use crate::state::AppState;

pub const MAIN_WINDOW: &str = "main";

/// Passed by the login item: start in the tray, without the window.
pub const HIDDEN_ARG: &str = "--hidden";

pub fn run() {
    tauri::Builder::default()
        // A second launch focuses the running app instead of starting another bunker. It must be
        // the first plugin; with its deep-link feature it hands a clicked link to this instance.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN_ARG]),
        ))
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let mut gooti = Gooti::open(&data_dir, Box::new(OsKeychain::new()));
            let unlocked = gooti.unlock_at_start();
            let state = AppState::new(gooti, WebSocketRelays::default());
            state.watch_backup(app.handle());
            if unlocked {
                state.on_unlocked(app.handle());
            }
            autolock::start(app.handle(), &state);
            backup_timer::start(app.handle(), &state);
            app.manage(state);
            links::start(app.handle());
            let has_tray = tray::create(app.handle())?;
            let state = app.state::<AppState>();
            state.set_has_tray(has_tray);
            // Started at login: stay in the tray. Without a tray, show the window anyway.
            let hidden = std::env::args().any(|arg| arg == HIDDEN_ARG);
            if !(hidden && has_tray) {
                show_main_window(app.handle());
            }
            if state.gooti().settings().check_updates {
                updates::check_at_start(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window keeps the bunker running, when the tray can bring it back.
            if window.label() == MAIN_WINDOW
                && let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window
                    .app_handle()
                    .try_state::<AppState>()
                    .is_some_and(|state| state.hide_on_close())
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running Gooti");
}

pub fn show_main_window<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Every command the UI may call. Shared with the IPC tests.
pub fn invoke_handler<R: Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static
{
    tauri::generate_handler![
        commands::app_info,
        commands::activity_touch,
        commands::vault_status,
        commands::vault_create,
        commands::vault_unlock,
        commands::vault_lock,
        commands::vault_reset,
        commands::vault_change_password,
        commands::identities_list,
        commands::identity_get,
        commands::identity_create,
        commands::identity_rename,
        commands::identity_set_custody,
        commands::identity_serve_here,
        commands::identity_set_color,
        commands::identity_delete,
        commands::identity_reveal_nsec,
        commands::identity_export_ncryptsec,
        commands::settings_get,
        commands::settings_set_unlock_at_login,
        commands::settings_change_lock,
        commands::settings_change_app,
        commands::autostart_get,
        commands::autostart_set,
        commands::update_check,
        commands::update_install,
        commands::open_releases,
        commands::open_link,
        commands::connect_link_take,
        commands::backup_info,
        commands::backup_default_relays,
        commands::backup_generate_passphrase,
        commands::backup_check_passphrase,
        commands::backup_enable,
        commands::backup_join,
        commands::backup_now,
        commands::backup_busy,
        commands::backup_set_relays,
        commands::backup_set_auto,
        commands::backup_disable,
        commands::vault_restore,
        commands::bunker_info,
        commands::bunker_pending,
        commands::bunker_link,
        commands::bunker_connect,
        commands::bunker_decide,
        commands::bunker_revoke,
        commands::bunker_set_paused,
        commands::bunker_set_relays,
        commands::bunker_set_app_relays,
        commands::bunker_connection,
        commands::bunker_set_trust,
        commands::bunker_forget,
        commands::activity_list,
        commands::activity_clear,
    ]
}

#[cfg(test)]
mod ipc_tests;
