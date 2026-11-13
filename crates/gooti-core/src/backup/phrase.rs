//! The 12-word passphrase and the two keys it gives. Port of `backup/phrase.ts`.

use bip39::{Language, Mnemonic};
use hkdf::Hkdf;
use nostr::key::{Keys, SecretKey};
use sha2::Sha256;
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

use crate::crypto::random_bytes;
use crate::error::{Error, Result};

/// HKDF salt. The phrase seed is already high entropy; this only separates the two keys.
const HKDF_SALT: &[u8] = b"gooti-backup-v1";
pub const SHARED_SIGN_INFO: &str = "gooti-shared-sign-v1";
pub const SHARED_ENCRYPT_INFO: &str = "gooti-shared-encrypt-v1";
/// Labels for the desktop record. Browsers keep only the shared keys, never the 12 words, so they
/// cannot derive these.
pub const DESKTOP_SIGN_INFO: &str = "gooti-desktop-sign-v1";
pub const DESKTOP_ENCRYPT_INFO: &str = "gooti-desktop-encrypt-v1";

pub struct BackupKeys {
    /// secp256k1 key for the kind 30078 event. Not an identity key.
    pub signing: Keys,
    /// AES-256-GCM key for the vault blob.
    pub encryption_key: Zeroizing<[u8; 32]>,
}

impl BackupKeys {
    pub fn pubkey_hex(&self) -> String {
        self.signing.public_key().to_hex()
    }

    pub fn from_hex(signing_hex: &str, encryption_hex: &str) -> Result<BackupKeys> {
        let secret = SecretKey::from_hex(signing_hex).map_err(|_| Error::UnreadableVault)?;
        let mut encryption_key = Zeroizing::new([0u8; 32]);
        decode_hex_into(encryption_hex, encryption_key.as_mut())?;
        Ok(BackupKeys {
            signing: Keys::new(secret),
            encryption_key,
        })
    }

    pub fn signing_hex(&self) -> Zeroizing<String> {
        Zeroizing::new(self.signing.secret_key().to_secret_hex())
    }

    pub fn encryption_hex(&self) -> Zeroizing<String> {
        Zeroizing::new(hex(self.encryption_key.as_ref()))
    }
}

/// 12 English words from 128 random bits, like nostr-tools' `generateSeedWords`.
pub fn generate_passphrase() -> Zeroizing<String> {
    let entropy = random_bytes::<16>();
    let mnemonic = Mnemonic::from_entropy_in(Language::English, entropy.as_ref())
        .expect("16 bytes of entropy always make 12 words");
    Zeroizing::new(mnemonic.to_string())
}

/// NFKD, trimmed, single spaces. Case is kept: the extension rejects "Abandon" too.
pub fn normalize_passphrase(phrase: &str) -> Zeroizing<String> {
    let nfkd: Zeroizing<String> = Zeroizing::new(phrase.nfkd().collect());
    Zeroizing::new(nfkd.split_whitespace().collect::<Vec<_>>().join(" "))
}

pub fn is_passphrase(phrase: &str) -> bool {
    Mnemonic::parse_in_normalized(Language::English, &normalize_passphrase(phrase)).is_ok()
}

/// The signing key is HKDF, not NIP-06 account 0, so it is never an identity.
pub fn derive_shared_keys(phrase: &str) -> Result<BackupKeys> {
    derive_keys(phrase, SHARED_SIGN_INFO, SHARED_ENCRYPT_INFO)
}

/// The keys of the desktop record, which only Gooti Desktops read.
pub fn derive_desktop_keys(phrase: &str) -> Result<BackupKeys> {
    derive_keys(phrase, DESKTOP_SIGN_INFO, DESKTOP_ENCRYPT_INFO)
}

fn derive_keys(phrase: &str, sign_info: &str, encrypt_info: &str) -> Result<BackupKeys> {
    let normalized = normalize_passphrase(phrase);
    let mnemonic = Mnemonic::parse_in_normalized(Language::English, &normalized)
        .map_err(|_| Error::InvalidPassphrase)?;
    let seed = Zeroizing::new(mnemonic.to_seed_normalized(""));
    let encryption_key = hkdf(seed.as_ref(), encrypt_info);

    for attempt in 0..8 {
        let info = if attempt == 0 {
            sign_info.to_string()
        } else {
            format!("{sign_info}:{attempt}")
        };
        let candidate = hkdf(seed.as_ref(), &info);
        // A 32-byte string is not always a secp256k1 scalar. Try the next label.
        if let Ok(secret) = SecretKey::from_slice(candidate.as_ref()) {
            return Ok(BackupKeys {
                signing: Keys::new(secret),
                encryption_key,
            });
        }
    }
    Err(Error::InvalidPassphrase)
}

fn hkdf(seed: &[u8], info: &str) -> Zeroizing<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), seed);
    let mut out = Zeroizing::new([0u8; 32]);
    hk.expand(info.as_bytes(), out.as_mut())
        .expect("32 bytes is a valid HKDF-SHA256 length");
    out
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn decode_hex_into(text: &str, out: &mut [u8]) -> Result<()> {
    if text.len() != out.len() * 2 {
        return Err(Error::UnreadableVault);
    }
    for (i, byte) in out.iter_mut().enumerate() {
        *byte =
            u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(|_| Error::UnreadableVault)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_phrases_are_valid_twelve_words() {
        let phrase = generate_passphrase();
        assert_eq!(phrase.split(' ').count(), 12);
        assert!(is_passphrase(&phrase));
        assert_ne!(*phrase, *generate_passphrase());
    }

    #[test]
    fn keys_round_trip_through_hex() {
        let keys = derive_shared_keys(&generate_passphrase()).unwrap();
        let back = BackupKeys::from_hex(&keys.signing_hex(), &keys.encryption_hex()).unwrap();
        assert_eq!(back.pubkey_hex(), keys.pubkey_hex());
        assert_eq!(*back.encryption_key, *keys.encryption_key);
    }

    #[test]
    fn the_desktop_keys_differ_from_the_shared_keys() {
        let phrase = generate_passphrase();
        let shared = derive_shared_keys(&phrase).unwrap();
        let desktop = derive_desktop_keys(&phrase).unwrap();
        assert_ne!(shared.pubkey_hex(), desktop.pubkey_hex());
        assert_ne!(*shared.encryption_key, *desktop.encryption_key);
    }
}
