//! Settings that must work while the vault is locked, so they are stored in the clear.
//! Nothing secret goes in here.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::vault::write_atomic;

/// Choices for "Lock after". `None` means never.
pub const AUTO_LOCK_CHOICES: [Option<u32>; 5] = [None, Some(5), Some(15), Some(60), Some(240)];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The data key is kept in the OS keychain and the vault opens when Gooti starts.
    pub unlock_at_login: bool,
    /// Lock after this many minutes without use. A bunker cannot sign while locked,
    /// so this starts off.
    pub auto_lock_minutes: Option<u32>,
    /// Lock when the screen locks.
    pub lock_on_screen_lock: bool,
    /// Lock before the computer sleeps.
    pub lock_on_sleep: bool,
    /// Closing the window keeps Gooti in the tray, so the bunker keeps answering apps.
    /// Without a tray, closing quits either way.
    pub keep_running: bool,
    /// Look for a new version when Gooti starts.
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            unlock_at_login: false,
            auto_lock_minutes: None,
            lock_on_screen_lock: true,
            lock_on_sleep: true,
            keep_running: true,
            check_updates: true,
        }
    }
}

impl Settings {
    /// Missing or unreadable settings fall back to the defaults. They hold nothing that
    /// cannot be chosen again.
    pub fn load(path: &Path) -> Settings {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
            .map(Settings::sanitized)
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        write_atomic(
            path,
            &serde_json::to_vec_pretty(self).expect("settings serialize"),
        )
    }

    fn sanitized(mut self) -> Settings {
        if !AUTO_LOCK_CHOICES.contains(&self.auto_lock_minutes) {
            self.auto_lock_minutes = None;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_lock_on_screen_lock_and_sleep_but_not_after_idle() {
        let settings = Settings::default();
        assert!(!settings.unlock_at_login);
        assert_eq!(settings.auto_lock_minutes, None);
        assert!(settings.lock_on_screen_lock);
        assert!(settings.lock_on_sleep);
        assert!(settings.keep_running);
        assert!(settings.check_updates);
    }

    #[test]
    fn settings_from_an_older_version_keep_running_and_check_updates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, br#"{"unlockAtLogin": true}"#).unwrap();
        let loaded = Settings::load(&path);
        assert!(loaded.unlock_at_login);
        assert!(loaded.keep_running);
        assert!(loaded.check_updates);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = Settings {
            unlock_at_login: true,
            auto_lock_minutes: Some(15),
            lock_on_screen_lock: false,
            lock_on_sleep: true,
            keep_running: false,
            check_updates: true,
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
    }

    #[test]
    fn missing_damaged_or_odd_values_fall_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(Settings::load(&path), Settings::default());
        fs::write(&path, b"nope").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        fs::write(&path, br#"{"autoLockMinutes": 7, "lockOnSleep": false}"#).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.auto_lock_minutes, None);
        assert!(!loaded.lock_on_sleep);
        assert!(loaded.lock_on_screen_lock);
    }
}
