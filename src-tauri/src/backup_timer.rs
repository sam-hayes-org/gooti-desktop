//! "Back up automatically" also checks the relays every few minutes, so changes made in a
//! browser reach this desktop without waiting for the next unlock or local change.

use std::time::Duration;

use tauri::AppHandle;

use crate::state::AppState;

/// How often the relays are checked. A local change or an unlock starts the wait over.
pub const CHECK_EVERY: Duration = Duration::from_secs(5 * 60);
const TICK: Duration = Duration::from_secs(30);

pub fn start(app: &AppHandle, state: &AppState) {
    let app = app.clone();
    let state = state.clone();
    std::thread::Builder::new()
        .name("gooti-backup-check".into())
        .spawn(move || {
            loop {
                std::thread::sleep(TICK);
                // Locked, or with automatic backup off, this is false.
                let on = state.gooti().vault().auto_backup();
                if due(on, state.since_last_backup()) {
                    state.check_backup(&app);
                }
            }
        })
        .expect("could not start the backup check");
}

/// Whether the regular check should run now.
pub fn due(auto_backup_on: bool, since_last_backup: Duration) -> bool {
    auto_backup_on && since_last_backup >= CHECK_EVERY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_only_when_on_and_five_minutes_passed() {
        assert!(due(true, CHECK_EVERY));
        assert!(due(true, CHECK_EVERY + Duration::from_secs(40)));
        assert!(!due(true, CHECK_EVERY - Duration::from_secs(1)));
        assert!(!due(false, CHECK_EVERY * 3));
    }
}
