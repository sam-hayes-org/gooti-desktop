//! Tray icon. Some Linux desktops (GNOME without an extension) show no tray,
//! so nothing may depend on it: the window stays the main way in.

use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

const OPEN: &str = "open";
const LOCK: &str = "lock";
const PAUSE: &str = "pause";
const QUIT: &str = "quit";

/// Creates the tray icon when this system can show one. Returns whether it did.
pub fn create(app: &AppHandle) -> tauri::Result<bool> {
    if !available() {
        eprintln!("Gooti: no tray library found, running without a tray icon.");
        return Ok(false);
    }
    let open = MenuItem::with_id(app, OPEN, "Open Gooti", true, None::<&str>)?;
    let lock = MenuItem::with_id(app, LOCK, "Lock now", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, PAUSE, "Pause all apps", true, false, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &lock, &pause, &quit])?;
    let pause_item = pause.clone();

    let mut builder = TrayIconBuilder::with_id("gooti")
        .tooltip("Gooti")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            OPEN => crate::show_main_window(app),
            LOCK => {
                if let Some(state) = app.try_state::<crate::state::AppState>() {
                    state.lock(app);
                }
            }
            PAUSE => {
                // The menu ticks the box itself; the vault decides. Locked: nothing to pause.
                let paused = app
                    .try_state::<crate::state::AppState>()
                    .and_then(|state| state.toggle_pause(app));
                let _ = pause_item.set_checked(paused.unwrap_or(false));
            }
            QUIT => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    // Windows: a left click opens the window, a right click the menu, as other tray apps do.
    #[cfg(windows)]
    {
        use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
        builder = builder
            .show_menu_on_left_click(false)
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    crate::show_main_window(tray.app_handle());
                }
            });
    }
    builder.build(app)?;
    Ok(true)
}

/// On Linux the tray loads libayatana-appindicator at runtime and panics when it is missing.
/// Check first, with the same library names it tries.
#[cfg(target_os = "linux")]
fn available() -> bool {
    const NAMES: [&str; 4] = [
        "libayatana-appindicator3.so.1",
        "libappindicator3.so.1",
        "libayatana-appindicator3.so",
        "libappindicator3.so",
    ];
    NAMES.iter().any(|name| {
        // SAFETY: loading a system library runs its initializers. These are the libraries the
        // tray would load anyway; the handle is dropped right away.
        unsafe { libloading::Library::new(name) }.is_ok()
    })
}

#[cfg(not(target_os = "linux"))]
fn available() -> bool {
    true
}
