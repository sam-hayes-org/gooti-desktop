//! Primitives for the local vault: Argon2id for the password, XChaCha20-Poly1305 for sealing.
//!
//! The local vault does not have to match the extension, so it uses Argon2id instead of PBKDF2.
//! Only the shared record (M2) must match the extension byte for byte.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::{Error, Result};

pub const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;

pub type SecretKeyBytes = Zeroizing<[u8; KEY_LEN]>;

/// Argon2id cost, stored with the vault so it can be raised later without breaking old vaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KdfParams {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

impl KdfParams {
    /// 64 MiB, 3 passes: above the OWASP minimum, about half a second on a laptop. Unlock is rare.
    pub const DEFAULT: KdfParams = KdfParams {
        memory_kib: 64 * 1024,
        iterations: 3,
        parallelism: 1,
    };

    /// Cheap settings so tests stay fast. Never used for a real vault.
    #[cfg(any(test, feature = "test-util"))]
    pub const TEST: KdfParams = KdfParams {
        memory_kib: 64,
        iterations: 1,
        parallelism: 1,
    };

    /// Rejects values a damaged or hostile file could use to exhaust memory or time.
    fn check(&self) -> Result<()> {
        let ok = (8..=1024 * 1024).contains(&self.memory_kib)
            && (1..=20).contains(&self.iterations)
            && (1..=16).contains(&self.parallelism);
        if ok {
            Ok(())
        } else {
            Err(Error::UnreadableVault)
        }
    }
}

pub fn random_bytes<const N: usize>() -> Zeroizing<[u8; N]> {
    let mut bytes = Zeroizing::new([0u8; N]);
    getrandom::fill(bytes.as_mut()).expect("the operating system has no random source");
    bytes
}

pub fn derive_key(password: &str, salt: &[u8], params: KdfParams) -> Result<SecretKeyBytes> {
    params.check()?;
    let argon_params = Params::new(
        params.memory_kib,
        params.iterations,
        params.parallelism,
        Some(KEY_LEN),
    )
    .map_err(|_| Error::UnreadableVault)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|_| Error::UnreadableVault)?;
    Ok(key)
}

/// Returns `nonce || ciphertext`. A fresh random nonce every call.
pub fn seal(key: &[u8; KEY_LEN], plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce = random_bytes::<NONCE_LEN>();
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(nonce.as_ref()),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .expect("XChaCha20-Poly1305 encryption cannot fail for in-memory buffers");
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(nonce.as_ref());
    out.extend_from_slice(&ciphertext);
    out
}

/// Opens `nonce || ciphertext`. `None` when the key is wrong or the data was changed.
pub fn open(key: &[u8; KEY_LEN], sealed: &[u8], aad: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if sealed.len() < NONCE_LEN {
        return None;
    }
    let (nonce, ciphertext) = sealed.split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .ok()
        .map(Zeroizing::new)
}

pub fn to_base64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

pub fn from_base64(text: &str) -> Result<Vec<u8>> {
    STANDARD.decode(text).map_err(|_| Error::UnreadableVault)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_then_open_returns_the_plaintext() {
        let key = random_bytes::<KEY_LEN>();
        let sealed = seal(&key, b"hello", b"aad");
        assert_eq!(open(&key, &sealed, b"aad").unwrap().as_slice(), b"hello");
    }

    #[test]
    fn the_same_plaintext_seals_differently_each_time() {
        let key = random_bytes::<KEY_LEN>();
        assert_ne!(seal(&key, b"hello", b""), seal(&key, b"hello", b""));
    }

    #[test]
    fn open_fails_with_a_wrong_key_a_wrong_aad_or_changed_bytes() {
        let key = random_bytes::<KEY_LEN>();
        let other = random_bytes::<KEY_LEN>();
        let mut sealed = seal(&key, b"hello", b"aad");
        assert!(open(&other, &sealed, b"aad").is_none());
        assert!(open(&key, &sealed, b"other").is_none());
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        assert!(open(&key, &sealed, b"aad").is_none());
        assert!(open(&key, &[1, 2, 3], b"aad").is_none());
    }

    #[test]
    fn derive_key_depends_on_password_and_salt() {
        let a = derive_key("password", b"salt-salt-salt-1", KdfParams::TEST).unwrap();
        let b = derive_key("password", b"salt-salt-salt-1", KdfParams::TEST).unwrap();
        let c = derive_key("Password", b"salt-salt-salt-1", KdfParams::TEST).unwrap();
        let d = derive_key("password", b"salt-salt-salt-2", KdfParams::TEST).unwrap();
        assert_eq!(*a, *b);
        assert_ne!(*a, *c);
        assert_ne!(*a, *d);
    }

    #[test]
    fn derive_key_rejects_hostile_cost_settings() {
        let huge = KdfParams {
            memory_kib: u32::MAX,
            iterations: 3,
            parallelism: 1,
        };
        assert!(matches!(
            derive_key("password", b"salt-salt-salt-1", huge),
            Err(Error::UnreadableVault)
        ));
    }
}
