//! "Unlock at login": the vault's data key in the OS keychain
//! (Secret Service on Linux, Keychain on macOS, Credential Manager on Windows).

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use gooti_core::{Error, KEY_LEN, Keychain, Result, SecretKeyBytes, Zeroizing};

const SERVICE: &str = "com.getgooti.desktop";
const ACCOUNT: &str = "vault-data-key";

pub struct OsKeychain {
    account: &'static str,
}

impl OsKeychain {
    pub fn new() -> Self {
        OsKeychain { account: ACCOUNT }
    }

    fn entry(&self) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, self.account).map_err(keychain_error)
    }
}

impl Keychain for OsKeychain {
    fn store(&self, data_key: &[u8; KEY_LEN]) -> Result<()> {
        let encoded = Zeroizing::new(STANDARD.encode(data_key));
        self.entry()?.set_password(&encoded).map_err(keychain_error)
    }

    fn load(&self) -> Result<Option<SecretKeyBytes>> {
        let encoded = match self.entry()?.get_password() {
            Ok(encoded) => Zeroizing::new(encoded),
            Err(keyring::Error::NoEntry) => return Ok(None),
            Err(error) => return Err(keychain_error(error)),
        };
        let bytes = Zeroizing::new(
            STANDARD
                .decode(encoded.as_bytes())
                .map_err(|_| Error::Keychain("the stored key is damaged".into()))?,
        );
        let key: [u8; KEY_LEN] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| Error::Keychain("the stored key is damaged".into()))?;
        Ok(Some(Zeroizing::new(key)))
    }

    fn delete(&self) -> Result<()> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(keychain_error(error)),
        }
    }
}

fn keychain_error(error: keyring::Error) -> Error {
    Error::Keychain(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Talks to the real keychain, so it only runs on request:
    /// `cargo test -p gooti-desktop -- --ignored`
    #[test]
    #[ignore]
    fn stores_loads_and_deletes_in_the_os_keychain() {
        let keychain = OsKeychain {
            account: "test-only-key",
        };
        keychain.delete().unwrap();
        assert!(keychain.load().unwrap().is_none());
        keychain.store(&[7u8; KEY_LEN]).unwrap();
        assert_eq!(*keychain.load().unwrap().unwrap(), [7u8; KEY_LEN]);
        keychain.delete().unwrap();
        assert!(keychain.load().unwrap().is_none());
    }
}
