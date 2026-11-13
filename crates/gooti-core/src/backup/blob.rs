//! The sealed vault: `version || 12-byte IV || AES-256-GCM ciphertext`, base64.
//! Port of `backup/blob.ts`.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::document::{SharedDocument, parse_shared_document};
use crate::crypto::random_bytes;
use crate::error::{Error, Result};

const VERSION: u8 = 1;
/// A decoded blob larger than this is rejected before decrypting.
const MAX_PACKED_BYTES: usize = 1_000_000;
/// The relay rejects base64 content above this many characters.
pub const BACKUP_CONTENT_LIMIT: usize = 1_000_000;

/// Same document, different blob every call.
pub fn seal_shared_document(
    document: &SharedDocument,
    encryption_key: &[u8; 32],
) -> Result<String> {
    seal_plaintext(&shared_plaintext(document), encryption_key)
}

/// Seals any plaintext the same way. The desktop record uses it too.
pub(crate) fn seal_plaintext(plaintext: &[u8], encryption_key: &[u8; 32]) -> Result<String> {
    if content_chars_for(plaintext.len()) > BACKUP_CONTENT_LIMIT {
        return Err(Error::VaultTooLarge);
    }
    let iv = random_bytes::<12>();
    let cipher = Aes256Gcm::new(encryption_key.into());
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(iv.as_ref()), plaintext)
        .expect("AES-GCM encryption cannot fail for in-memory buffers");
    let mut packed = Vec::with_capacity(1 + 12 + ciphertext.len());
    packed.push(VERSION);
    packed.extend_from_slice(iv.as_ref());
    packed.extend_from_slice(&ciphertext);
    Ok(STANDARD.encode(packed))
}

/// `Ok(None)` for an older document (not v2): the caller publishes the current one over it.
/// `Err` when the key is wrong or the blob was changed.
pub fn open_shared_document(
    payload: &str,
    encryption_key: &[u8; 32],
) -> Result<Option<SharedDocument>> {
    let plaintext = open_plaintext(payload, encryption_key)?;
    let parsed: Value = serde_json::from_slice(&plaintext).map_err(|_| Error::InvalidBackup)?;
    if parsed.get("v") != Some(&Value::from(2)) {
        return Ok(None);
    }
    parse_shared_document(parsed)
        .map(Some)
        .ok_or(Error::InvalidBackup)
}

/// `Err` when the key is wrong or the blob was changed.
pub(crate) fn open_plaintext(
    payload: &str,
    encryption_key: &[u8; 32],
) -> Result<Zeroizing<Vec<u8>>> {
    let packed = STANDARD.decode(payload).map_err(|_| Error::InvalidBackup)?;
    if packed.len() < 1 + 12 + 16 || packed.len() > MAX_PACKED_BYTES || packed[0] != VERSION {
        return Err(Error::InvalidBackup);
    }
    let cipher = Aes256Gcm::new(encryption_key.into());
    Ok(Zeroizing::new(
        cipher
            .decrypt(Nonce::from_slice(&packed[1..13]), &packed[13..])
            .map_err(|_| Error::InvalidBackup)?,
    ))
}

/// Characters of base64 content the relay will store for this document.
pub fn shared_content_chars(document: &SharedDocument) -> usize {
    content_chars_for(shared_plaintext(document).len())
}

/// Encryption adds 29 bytes (version, IV, tag); base64 makes it a third longer.
fn content_chars_for(plaintext_len: usize) -> usize {
    4 * (plaintext_len + 29).div_ceil(3)
}

fn shared_plaintext(document: &SharedDocument) -> Zeroizing<Vec<u8>> {
    let value = json!({
        "v": document.v,
        "identities": document.identities,
        "permissions": document.permissions,
        "relays": document.relays,
        "sites": document.sites,
    });
    Zeroizing::new(serde_json::to_vec(&value).expect("a shared document serializes"))
}
