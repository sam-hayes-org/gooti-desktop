//! Gooti core: everything that touches keys lives here, never in the UI.
//!
//! The desktop app (`src-tauri`) and a later server app are thin shells around this crate.

pub mod activity;
pub mod backup;
pub mod bunker;
mod color;
mod crypto;
mod error;
mod gooti;
mod identity;
mod keychain;
mod settings;
mod time;
mod vault;

pub use color::{color_for_pubkey, color_from_pubkey};
pub use crypto::{KEY_LEN, KdfParams, SecretKeyBytes};
pub use error::{Error, MIN_PASSWORD_LENGTH, RelayFailure, Result};
pub use gooti::{AppSettingsChange, Gooti, LockSettingsChange, SETTINGS_FILE, VAULT_FILE};
pub use identity::{Custody, IdentitySummary, NewKey};
pub use keychain::{Keychain, MemoryKeychain};
pub use settings::{AUTO_LOCK_CHOICES, Settings};
pub use vault::{Applied, BackupInfo, Vault, VaultStatus};
pub use zeroize::Zeroizing;

/// Milliseconds since 1970, as stored in Activity and the vault.
pub fn time_now_ms() -> i64 {
    time::now_ms()
}

use serde::Serialize;

/// Version of this build, shown in the app.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Release channel. Alpha until the first stable release.
pub const CHANNEL: &str = "alpha";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: &'static str,
    pub channel: &'static str,
}

pub fn app_info() -> AppInfo {
    AppInfo {
        version: VERSION,
        channel: CHANNEL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_reports_this_build() {
        let info = app_info();
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(info.channel, "alpha");
    }

    #[test]
    fn app_info_serializes_in_camel_case_for_the_ui() {
        let json = serde_json::to_value(app_info()).unwrap();
        assert_eq!(json["channel"], "alpha");
        assert_eq!(json["version"], VERSION);
    }
}
