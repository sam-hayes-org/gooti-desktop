//! The small window for bunker requests waiting for Approve or Reject, and the notification
//! that comes with each new one.

use gooti_core::bunker::handler::Ask;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_notification::NotificationExt;

pub const APPROVAL_WINDOW: &str = "approval";

/// Shows the approval window, creating it the first time.
pub fn open<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(APPROVAL_WINDOW) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return;
    }
    // The UI reads the window label and shows only the approval panel.
    let built =
        WebviewWindowBuilder::new(app, APPROVAL_WINDOW, WebviewUrl::App("index.html".into()))
            .title("Gooti")
            .inner_size(460.0, 620.0)
            .min_inner_size(400.0, 480.0)
            .center()
            .always_on_top(true)
            .focused(true)
            .background_color(tauri::window::Color(0x16, 0x1c, 0x26, 0xff))
            // Dark GTK theme, so native parts such as the open list of a select are dark too.
            .theme(Some(tauri::Theme::Dark))
            .build();
    if let Err(error) = built {
        eprintln!("Gooti: cannot open the approval window: {error}");
        // The main window has the same panel.
        crate::show_main_window(app);
    }
}

/// Closes the approval window when nothing waits any more.
pub fn close<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(APPROVAL_WINDOW) {
        let _ = window.close();
    }
}

/// "Coracle wants to sign a note", as a system notification.
pub fn notify<R: Runtime>(app: &AppHandle<R>, app_name: &str, ask: &Ask) {
    let body = format!("{app_name} wants to {}.", action(ask));
    let _ = app
        .notification()
        .builder()
        .title("Gooti")
        .body(body)
        .show();
}

fn action(ask: &Ask) -> String {
    match ask.method.as_str() {
        "sign_event" => match ask.details.kind {
            Some(0) => "change your profile".into(),
            Some(1) => "sign a note".into(),
            Some(3) => "change your follow list".into(),
            Some(5) => "delete something".into(),
            Some(7) => "sign a reaction".into(),
            Some(kind) => format!("sign an event of kind {kind}"),
            None => "sign an event".into(),
        },
        method if method.ends_with("_encrypt") => "encrypt a message".into(),
        _ => "decrypt a message".into(),
    }
}
