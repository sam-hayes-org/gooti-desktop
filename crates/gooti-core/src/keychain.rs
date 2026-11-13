//! Where "Unlock at login" keeps the vault's data key. The desktop app uses the OS keychain
//! (Secret Service on Linux). The core only sees this trait.

use std::sync::Mutex;

use zeroize::Zeroizing;

use crate::crypto::{KEY_LEN, SecretKeyBytes};
use crate::error::Result;

pub trait Keychain: Send + Sync {
    fn store(&self, data_key: &[u8; KEY_LEN]) -> Result<()>;
    /// `Ok(None)` when nothing is stored.
    fn load(&self) -> Result<Option<SecretKeyBytes>>;
    /// Succeeds when nothing is stored.
    fn delete(&self) -> Result<()>;
}

/// In memory, for tests and for systems without a keychain.
#[derive(Default)]
pub struct MemoryKeychain {
    key: Mutex<Option<SecretKeyBytes>>,
}

impl Keychain for MemoryKeychain {
    fn store(&self, data_key: &[u8; KEY_LEN]) -> Result<()> {
        *self.key.lock().unwrap() = Some(Zeroizing::new(*data_key));
        Ok(())
    }

    fn load(&self) -> Result<Option<SecretKeyBytes>> {
        Ok(self.key.lock().unwrap().clone())
    }

    fn delete(&self) -> Result<()> {
        *self.key.lock().unwrap() = None;
        Ok(())
    }
}
