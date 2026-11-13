//! `nostrconnect://` links clicked in a browser or another app. They open "Connect new app" with
//! the link filled in; the user still picks the identity and presses Connect.

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_deep_link::DeepLinkExt;

use crate::state::AppState;

/// A link arrived. The UI takes it with `connect_link_take` while unlocked.
pub const CONNECT_LINK_EVENT: &str = "connect-link";

/// Longer than any real link; keeps a crafted argument from filling memory or the UI.
const MAX_LINK_LEN: usize = 8 * 1024;

pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let links = app.deep_link();
    // An AppImage is not installed, so it registers itself as the handler. Installed packages
    // declare the handler in their desktop file, and the Windows installer in the registry; a dev
    // build registers nothing.
    #[cfg(target_os = "linux")]
    if app.env().appimage.is_some() {
        let _ = links.register_all();
    }
    if let Ok(Some(urls)) = links.get_current() {
        received(app, urls.iter().map(|u| u.as_str()));
    }
    let handle = app.clone();
    links.on_open_url(move |event| {
        let urls = event.urls();
        received(&handle, urls.iter().map(|u| u.as_str()));
    });
}

fn received<'a, R: Runtime>(app: &AppHandle<R>, urls: impl Iterator<Item = &'a str>) {
    let Some(link) = urls.filter_map(connect_link).next() else {
        return;
    };
    app.state::<AppState>().set_connect_link(link);
    crate::show_main_window(app);
    let _ = app.emit(CONNECT_LINK_EVENT, ());
}

/// A `nostrconnect://` link, or `None` for anything else.
pub fn connect_link(url: &str) -> Option<String> {
    let url = url.trim();
    let scheme = url.get(..15)?;
    (scheme.eq_ignore_ascii_case("nostrconnect://") && url.len() <= MAX_LINK_LEN)
        .then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::connect_link;

    #[test]
    fn takes_only_nostrconnect_links() {
        let link = "nostrconnect://abc?relay=wss%3A%2F%2Fr.example&secret=x";
        assert_eq!(connect_link(link).as_deref(), Some(link));
        assert_eq!(
            connect_link(" NOSTRCONNECT://abc ").as_deref(),
            Some("NOSTRCONNECT://abc")
        );
        assert_eq!(connect_link("bunker://abc"), None);
        assert_eq!(connect_link("--hidden"), None);
        assert_eq!(connect_link("nostr"), None);
        assert_eq!(
            connect_link(&format!("nostrconnect://{}", "a".repeat(9000))),
            None
        );
    }
}
