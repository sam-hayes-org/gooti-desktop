//! The vault, its settings and the keychain together: the rules that span them.

use std::path::{Path, PathBuf};

use crate::crypto::KdfParams;
use crate::error::{Error, Result};
use crate::keychain::Keychain;
use crate::settings::{AUTO_LOCK_CHOICES, Settings};
use crate::vault::Vault;

pub const VAULT_FILE: &str = "vault.json";
pub const SETTINGS_FILE: &str = "settings.json";

/// Changes to the lock settings. `None` leaves a value as it is.
#[derive(Debug, Default, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockSettingsChange {
    /// `Some(None)` turns the idle lock off.
    #[serde(default, with = "double_option")]
    pub auto_lock_minutes: Option<Option<u32>>,
    pub lock_on_screen_lock: Option<bool>,
    pub lock_on_sleep: Option<bool>,
}

/// Changes to how the app behaves. `None` leaves a value as it is.
#[derive(Debug, Default, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsChange {
    pub keep_running: Option<bool>,
    pub check_updates: Option<bool>,
}

pub struct Gooti {
    vault: Vault,
    settings: Settings,
    settings_path: PathBuf,
    keychain: Box<dyn Keychain>,
}

impl Gooti {
    /// Opens Gooti's data folder. Nothing is unlocked yet.
    pub fn open(dir: &Path, keychain: Box<dyn Keychain>) -> Gooti {
        Self::open_with_kdf(dir, keychain, KdfParams::DEFAULT)
    }

    pub fn open_with_kdf(dir: &Path, keychain: Box<dyn Keychain>, kdf: KdfParams) -> Gooti {
        let settings_path = dir.join(SETTINGS_FILE);
        Gooti {
            vault: Vault::with_kdf(dir.join(VAULT_FILE), kdf),
            settings: Settings::load(&settings_path),
            settings_path,
            keychain,
        }
    }

    pub fn vault(&self) -> &Vault {
        &self.vault
    }

    pub fn vault_mut(&mut self) -> &mut Vault {
        &mut self.vault
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// At start: opens the vault with the keychain's data key when "Unlock at login" is on.
    /// A key that no longer fits (the vault was replaced) is removed and the setting turned off.
    /// Returns whether the vault is now unlocked.
    pub fn unlock_at_start(&mut self) -> bool {
        if !self.settings.unlock_at_login || self.vault.is_unlocked() {
            return self.vault.is_unlocked();
        }
        match self.keychain.load() {
            Ok(Some(key)) => match self.vault.unlock_with_data_key(&key) {
                Ok(()) => true,
                Err(Error::InvalidPassword) => {
                    let _ = self.set_unlock_at_login(false);
                    false
                }
                Err(_) => false,
            },
            // The keychain may still be locked or missing. Leave the setting for next time.
            Ok(None) | Err(_) => false,
        }
    }

    /// Turning it on needs an unlocked vault. A keychain failure leaves the setting as it was.
    pub fn set_unlock_at_login(&mut self, on: bool) -> Result<()> {
        if on {
            let key = self.vault.data_key()?;
            self.keychain.store(&key)?;
        } else {
            self.keychain.delete()?;
        }
        self.settings.unlock_at_login = on;
        self.settings.save(&self.settings_path)
    }

    pub fn change_lock_settings(&mut self, change: LockSettingsChange) -> Result<Settings> {
        let mut next = self.settings.clone();
        if let Some(minutes) = change.auto_lock_minutes {
            if !AUTO_LOCK_CHOICES.contains(&minutes) {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "not a lock time Gooti offers",
                )));
            }
            next.auto_lock_minutes = minutes;
        }
        if let Some(on) = change.lock_on_screen_lock {
            next.lock_on_screen_lock = on;
        }
        if let Some(on) = change.lock_on_sleep {
            next.lock_on_sleep = on;
        }
        next.save(&self.settings_path)?;
        self.settings = next;
        Ok(self.settings.clone())
    }

    pub fn change_app_settings(&mut self, change: AppSettingsChange) -> Result<Settings> {
        let mut next = self.settings.clone();
        if let Some(on) = change.keep_running {
            next.keep_running = on;
        }
        if let Some(on) = change.check_updates {
            next.check_updates = on;
        }
        next.save(&self.settings_path)?;
        self.settings = next;
        Ok(self.settings.clone())
    }

    /// Deletes the vault, the keychain entry and the settings.
    pub fn reset(&mut self) -> Result<()> {
        self.vault.reset()?;
        let _ = self.keychain.delete();
        self.settings = Settings::default();
        match std::fs::remove_file(&self.settings_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

/// Tells a missing field apart from an explicit `null`.
mod double_option {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        Option::<T>::deserialize(deserializer).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::keychain::MemoryKeychain;
    use crate::vault::VaultStatus;

    /// Lets a test look into the keychain after handing it to `Gooti`.
    #[derive(Clone, Default)]
    struct SharedKeychain(Arc<MemoryKeychain>);

    impl Keychain for SharedKeychain {
        fn store(&self, key: &[u8; 32]) -> Result<()> {
            self.0.store(key)
        }
        fn load(&self) -> Result<Option<crate::crypto::SecretKeyBytes>> {
            self.0.load()
        }
        fn delete(&self) -> Result<()> {
            self.0.delete()
        }
    }

    struct FailingKeychain;

    impl Keychain for FailingKeychain {
        fn store(&self, _: &[u8; 32]) -> Result<()> {
            Err(Error::Keychain("no Secret Service".into()))
        }
        fn load(&self) -> Result<Option<crate::crypto::SecretKeyBytes>> {
            Err(Error::Keychain("no Secret Service".into()))
        }
        fn delete(&self) -> Result<()> {
            Ok(())
        }
    }

    fn open(dir: &tempfile::TempDir, keychain: impl Keychain + 'static) -> Gooti {
        Gooti::open_with_kdf(dir.path(), Box::new(keychain), KdfParams::TEST)
    }

    #[test]
    fn unlock_at_login_opens_the_vault_on_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        let keychain = SharedKeychain::default();
        let mut gooti = open(&dir, keychain.clone());
        gooti.vault_mut().create("1234").unwrap();
        gooti.set_unlock_at_login(true).unwrap();
        assert!(keychain.load().unwrap().is_some());

        let mut next = open(&dir, keychain.clone());
        assert_eq!(next.vault().status(), VaultStatus::Locked);
        assert!(next.unlock_at_start());
        assert_eq!(next.vault().status(), VaultStatus::Unlocked);
    }

    #[test]
    fn without_unlock_at_login_the_vault_stays_locked() {
        let dir = tempfile::tempdir().unwrap();
        let keychain = SharedKeychain::default();
        let mut gooti = open(&dir, keychain.clone());
        gooti.vault_mut().create("1234").unwrap();
        // A leftover key must not be used while the setting is off.
        keychain.store(&gooti.vault().data_key().unwrap()).unwrap();

        let mut next = open(&dir, keychain);
        assert!(!next.unlock_at_start());
        assert_eq!(next.vault().status(), VaultStatus::Locked);
    }

    #[test]
    fn turning_it_on_needs_an_unlocked_vault() {
        let dir = tempfile::tempdir().unwrap();
        let mut gooti = open(&dir, MemoryKeychain::default());
        gooti.vault_mut().create("1234").unwrap();
        gooti.vault_mut().lock();
        assert!(matches!(
            gooti.set_unlock_at_login(true),
            Err(Error::Locked)
        ));
        assert!(!gooti.settings().unlock_at_login);
    }

    #[test]
    fn a_keychain_failure_leaves_the_setting_off() {
        let dir = tempfile::tempdir().unwrap();
        let mut gooti = open(&dir, FailingKeychain);
        gooti.vault_mut().create("1234").unwrap();
        let error = gooti.set_unlock_at_login(true).unwrap_err();
        assert_eq!(error.code(), "keychain");
        assert!(!gooti.settings().unlock_at_login);
        assert!(!Settings::load(&dir.path().join(SETTINGS_FILE)).unlock_at_login);
    }

    #[test]
    fn a_stale_keychain_key_is_dropped_and_the_setting_turned_off() {
        let dir = tempfile::tempdir().unwrap();
        let keychain = SharedKeychain::default();
        let mut gooti = open(&dir, keychain.clone());
        gooti.vault_mut().create("1234").unwrap();
        gooti.set_unlock_at_login(true).unwrap();
        keychain.store(&[9u8; 32]).unwrap();

        let mut next = open(&dir, keychain.clone());
        assert!(!next.unlock_at_start());
        assert!(!next.settings().unlock_at_login);
        assert!(keychain.load().unwrap().is_none());
    }

    #[test]
    fn turning_it_off_removes_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let keychain = SharedKeychain::default();
        let mut gooti = open(&dir, keychain.clone());
        gooti.vault_mut().create("1234").unwrap();
        gooti.set_unlock_at_login(true).unwrap();
        gooti.set_unlock_at_login(false).unwrap();
        assert!(keychain.load().unwrap().is_none());
    }

    #[test]
    fn lock_settings_change_only_what_is_given() {
        let dir = tempfile::tempdir().unwrap();
        let mut gooti = open(&dir, MemoryKeychain::default());
        let settings = gooti
            .change_lock_settings(LockSettingsChange {
                auto_lock_minutes: Some(Some(15)),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(settings.auto_lock_minutes, Some(15));
        assert!(settings.lock_on_sleep);

        let settings = gooti
            .change_lock_settings(LockSettingsChange {
                auto_lock_minutes: Some(None),
                lock_on_sleep: Some(false),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(settings.auto_lock_minutes, None);
        assert!(!settings.lock_on_sleep);
        assert_eq!(Settings::load(&dir.path().join(SETTINGS_FILE)), settings);

        assert!(
            gooti
                .change_lock_settings(LockSettingsChange {
                    auto_lock_minutes: Some(Some(7)),
                    ..Default::default()
                })
                .is_err()
        );
    }

    #[test]
    fn lock_settings_change_reads_null_as_off_and_missing_as_unchanged() {
        let off: LockSettingsChange = serde_json::from_str(r#"{"autoLockMinutes": null}"#).unwrap();
        assert_eq!(off.auto_lock_minutes, Some(None));
        let unchanged: LockSettingsChange =
            serde_json::from_str(r#"{"lockOnSleep": true}"#).unwrap();
        assert_eq!(unchanged.auto_lock_minutes, None);
    }

    #[test]
    fn app_settings_change_only_what_is_given() {
        let dir = tempfile::tempdir().unwrap();
        let mut gooti = open(&dir, MemoryKeychain::default());
        let settings = gooti
            .change_app_settings(AppSettingsChange {
                keep_running: Some(false),
                ..Default::default()
            })
            .unwrap();
        assert!(!settings.keep_running);
        assert!(settings.check_updates);
        assert_eq!(Settings::load(&dir.path().join(SETTINGS_FILE)), settings);
    }

    #[test]
    fn reset_removes_vault_keychain_entry_and_settings() {
        let dir = tempfile::tempdir().unwrap();
        let keychain = SharedKeychain::default();
        let mut gooti = open(&dir, keychain.clone());
        gooti.vault_mut().create("1234").unwrap();
        gooti.set_unlock_at_login(true).unwrap();
        gooti.reset().unwrap();
        assert_eq!(gooti.vault().status(), VaultStatus::NoVault);
        assert!(keychain.load().unwrap().is_none());
        assert_eq!(gooti.settings(), &Settings::default());
        assert!(!dir.path().join(SETTINGS_FILE).exists());
    }
}
