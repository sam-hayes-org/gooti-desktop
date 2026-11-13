//! Identities as stored in the vault, and what the UI may see of them.

use nostr::key::{Keys, PublicKey, SecretKey};
use nostr::nips::nip19::{FromBech32, ToBech32};
use nostr::nips::nip49::{EncryptedSecretKey, KeySecurity};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::backup::document::SharedBunker;
use crate::backup::state::LiveIdentity;
use crate::error::{Error, Result, check_password};

/// NIP-49 scrypt cost for exported keys. 16 is the NIP's own example and the nostr crate's default.
const NCRYPTSEC_LOG_N: u8 = 16;

/// One identity inside the sealed vault. Revisions for backup live in the shared record,
/// like the extension's stamps, not here.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredIdentity {
    pub id: String,
    /// ISO time, like the extension's `createdAt`.
    pub created_at: String,
    pub nick: String,
    /// Hex public key.
    pub pubkey: String,
    /// Hex private key. Absent on a bunker identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privkey: Option<String>,
    /// Optional `#rrggbb`. The UI derives one from the pubkey when absent, like the extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// A bunker identity from a browser: the key stays in that bunker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bunker: Option<SharedBunker>,
    /// Who holds the private key once backup is on. Older vaults are `Shared`.
    #[serde(default)]
    pub custody: Custody,
    /// When the key last moved to this desktop or back into backup (ms). Backup carries it as
    /// `custodyAt`, so an older copy of the key cannot undo a move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custody_at: Option<f64>,
    /// For `Desktop` custody: the session key browsers use to reach this desktop's bunker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browsers: Option<BrowserLink>,
    /// Written by the first alpha for a deleted identity. Such rows are dropped when read.
    #[serde(default, skip_serializing)]
    pub deleted_at: Option<i64>,
}

/// Who holds an identity's private key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Custody {
    /// Backup carries the key: every device with the passphrase holds it.
    #[default]
    #[serde(alias = "synced")]
    Shared,
    /// The key stays on this desktop. Backup carries a bunker pointer; browsers sign through it.
    Desktop,
}

/// How browsers reach a `Desktop` identity: a session key made here and shared inside the
/// pointer, and the extension's "Sites and the bunker" choice.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserLink {
    pub client_secret_hex: String,
    /// `None` is the extension's default: ask which site.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask_site: Option<bool>,
    /// The signer key the pointer names. `None`: the identity's first signer key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer_pubkey: Option<String>,
}

impl Drop for BrowserLink {
    fn drop(&mut self) {
        self.client_secret_hex.zeroize();
    }
}

impl Drop for StoredIdentity {
    fn drop(&mut self) {
        self.privkey.zeroize();
        if let Some(bunker) = self.bunker.as_mut() {
            bunker.client_secret_key.zeroize();
        }
    }
}

impl std::fmt::Debug for StoredIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredIdentity")
            .field("id", &self.id)
            .field("nick", &self.nick)
            .field("pubkey", &self.pubkey)
            .field("privkey", &self.privkey.as_ref().map(|_| "<hidden>"))
            .field("bunker", &self.bunker.as_ref().map(|_| "<bunker>"))
            .finish()
    }
}

impl StoredIdentity {
    pub fn keys(&self) -> Result<Keys> {
        let privkey = self.privkey.as_deref().ok_or(Error::NoPrivateKey)?;
        let secret = SecretKey::from_hex(privkey).map_err(|_| Error::UnreadableVault)?;
        Ok(Keys::new(secret))
    }

    pub fn summary(&self) -> IdentitySummary {
        IdentitySummary {
            id: self.id.clone(),
            nick: self.nick.clone(),
            pubkey: self.pubkey.clone(),
            npub: npub(&self.pubkey),
            color: self.color.clone(),
            created_at: self.created_at.clone(),
            has_private_key: self.privkey.is_some(),
            is_bunker: self.bunker.is_some(),
            key_missing: self.privkey.is_none()
                && self.bunker.is_some()
                && self.custody_at.is_some(),
            served_elsewhere: false,
            custody: self.custody,
        }
    }

    pub fn live(&self) -> LiveIdentity {
        LiveIdentity {
            pubkey: self.pubkey.clone(),
            privkey: self.privkey.clone(),
            nick: self.nick.clone(),
            created_at: self.created_at.clone(),
            color: self.color.clone(),
            bunker: self.bunker.clone(),
            custody_at: self.custody_at,
        }
    }

    pub fn from_live(id: String, live: LiveIdentity) -> StoredIdentity {
        StoredIdentity {
            id,
            created_at: live.created_at,
            nick: live.nick,
            pubkey: live.pubkey,
            privkey: live.privkey,
            color: live.color,
            bunker: live.bunker,
            custody: Custody::Shared,
            custody_at: live.custody_at,
            browsers: None,
            deleted_at: None,
        }
    }
}

/// What the UI sees. Never a private key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentitySummary {
    pub id: String,
    pub nick: String,
    pub pubkey: String,
    pub npub: String,
    pub color: Option<String>,
    pub created_at: String,
    pub has_private_key: bool,
    /// From a browser that links it to a bunker. This desktop cannot sign with it.
    pub is_bunker: bool,
    /// A Gooti Desktop pointer whose key is in no desktop record: no Gooti Desktop with this
    /// passphrase holds the key. It is lost, unless someone saved it.
    pub key_missing: bool,
    /// This desktop holds the key, but another Gooti Desktop answers apps and browsers for it.
    pub served_elsewhere: bool,
    pub custody: Custody,
}

/// How a new identity gets its key.
pub enum NewKey<'a> {
    Generate,
    /// nsec, hex, or ncryptsec (then `password` is needed).
    Import {
        key: &'a str,
        password: Option<&'a str>,
    },
}

pub fn keys_for(new_key: NewKey<'_>) -> Result<Keys> {
    match new_key {
        NewKey::Generate => Ok(Keys::generate()),
        NewKey::Import { key, password } => parse_private_key(key, password).map(Keys::new),
    }
}

fn parse_private_key(input: &str, password: Option<&str>) -> Result<SecretKey> {
    let input = Zeroizing::new(input.trim().to_lowercase());
    if input.starts_with("ncryptsec1") {
        let encrypted =
            EncryptedSecretKey::from_bech32(&input).map_err(|_| Error::InvalidPrivateKey)?;
        let password = password
            .filter(|p| !p.is_empty())
            .ok_or(Error::KeyPasswordRequired)?;
        return encrypted
            .decrypt(password)
            .map_err(|_| Error::InvalidKeyPassword);
    }
    SecretKey::parse(&input).map_err(|_| Error::InvalidPrivateKey)
}

pub fn nsec(keys: &Keys) -> Zeroizing<String> {
    Zeroizing::new(
        keys.secret_key()
            .to_bech32()
            .expect("a valid secret key always encodes"),
    )
}

pub fn ncryptsec(keys: &Keys, password: &str) -> Result<String> {
    check_password(password)?;
    let encrypted = EncryptedSecretKey::new(
        keys.secret_key(),
        password,
        NCRYPTSEC_LOG_N,
        KeySecurity::Unknown,
    )
    .map_err(|_| Error::InvalidPrivateKey)?;
    Ok(encrypted
        .to_bech32()
        .expect("an encrypted key always encodes"))
}

pub fn npub(pubkey_hex: &str) -> String {
    PublicKey::from_hex(pubkey_hex)
        .ok()
        .and_then(|key| key.to_bech32().ok())
        .unwrap_or_default()
}

pub fn normalize_nick(nick: &str) -> Result<String> {
    let nick = nick.trim();
    if nick.is_empty() {
        return Err(Error::NickRequired);
    }
    Ok(nick.to_string())
}

/// `#rrggbb` in lower case, the same rule as the extension's `readIdentityColor`.
pub fn normalize_color(color: &str) -> Result<String> {
    normalize_color_loose(color).ok_or(Error::InvalidColor)
}

/// `readIdentityColor`: trimmed and lower-cased, or `None` when it is not `#rrggbb`.
pub fn normalize_color_loose(color: &str) -> Option<String> {
    let color = color.trim().to_lowercase();
    let valid = color.len() == 7
        && color.starts_with('#')
        && color[1..].chars().all(|c| c.is_ascii_hexdigit());
    valid.then_some(color)
}

/// Hex public key of a hex private key, or `None` when it is not a valid key.
pub fn pubkey_for_privkey(privkey_hex: &str) -> Option<String> {
    SecretKey::from_hex(privkey_hex)
        .ok()
        .map(|secret| Keys::new(secret).public_key().to_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    // From NIP-19 and NIP-49.
    const NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
    const HEX: &str = "67dea2ed018072d675f5415ecfaed7d2597555e202d85b3d65ea4e58d2d92ffa";
    const NCRYPTSEC: &str = "ncryptsec1qgg9947rlpvqu76pj5ecreduf9jxhselq2nae2kghhvd5g7dgjtcxfqtd67p9m0w57lspw8gsq6yphnm8623nsl8xn9j4jdzz84zm3frztj3z7s35vpzmqf6ksu8r89qk5z2zxfmu5gv8th8wclt0h4p";

    #[test]
    fn imports_nsec_and_hex_as_the_same_key() {
        let from_nsec = keys_for(NewKey::Import {
            key: NSEC,
            password: None,
        })
        .unwrap();
        let from_hex = keys_for(NewKey::Import {
            key: HEX,
            password: None,
        })
        .unwrap();
        assert_eq!(from_nsec.public_key(), from_hex.public_key());
        assert_eq!(from_hex.secret_key().to_secret_hex(), HEX);
    }

    #[test]
    fn import_trims_and_ignores_case() {
        let keys = keys_for(NewKey::Import {
            key: &format!("  {}\n", NSEC.to_uppercase()),
            password: None,
        })
        .unwrap();
        assert_eq!(keys.secret_key().to_secret_hex(), HEX);
    }

    #[test]
    fn imports_the_nip49_test_vector() {
        let keys = keys_for(NewKey::Import {
            key: NCRYPTSEC,
            password: Some("nostr"),
        })
        .unwrap();
        assert_eq!(
            keys.secret_key().to_secret_hex(),
            "3501454135014541350145413501453fefb02227e449e57cf4d3a3ce05378683"
        );
    }

    #[test]
    fn ncryptsec_needs_the_right_password() {
        let missing = keys_for(NewKey::Import {
            key: NCRYPTSEC,
            password: None,
        });
        assert!(matches!(missing, Err(Error::KeyPasswordRequired)));
        let wrong = keys_for(NewKey::Import {
            key: NCRYPTSEC,
            password: Some("wrong"),
        });
        assert!(matches!(wrong, Err(Error::InvalidKeyPassword)));
    }

    #[test]
    fn rejects_text_that_is_not_a_key() {
        for input in ["", "hello", "npub1xyz", &HEX[..63]] {
            let result = keys_for(NewKey::Import {
                key: input,
                password: None,
            });
            assert!(matches!(result, Err(Error::InvalidPrivateKey)), "{input:?}");
        }
    }

    #[test]
    fn exported_ncryptsec_opens_with_its_password() {
        let keys = Keys::generate();
        let exported = ncryptsec(&keys, "secret").unwrap();
        assert!(exported.starts_with("ncryptsec1"));
        let back = keys_for(NewKey::Import {
            key: &exported,
            password: Some("secret"),
        })
        .unwrap();
        assert_eq!(back.public_key(), keys.public_key());
        assert!(matches!(
            ncryptsec(&keys, "abc"),
            Err(Error::PasswordTooShort)
        ));
    }

    #[test]
    fn nsec_encodes_the_secret_key() {
        let keys = keys_for(NewKey::Import {
            key: HEX,
            password: None,
        })
        .unwrap();
        assert_eq!(nsec(&keys).as_str(), NSEC);
    }

    #[test]
    fn colors_follow_the_extension_rule() {
        assert_eq!(normalize_color(" #1A2b3C ").unwrap(), "#1a2b3c");
        for bad in ["1a2b3c", "#1a2b3", "#1a2b3cd", "#gggggg", ""] {
            assert!(normalize_color(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn nick_is_trimmed_and_required() {
        assert_eq!(normalize_nick("  Sam ").unwrap(), "Sam");
        assert!(matches!(normalize_nick("   "), Err(Error::NickRequired)));
    }

    #[test]
    fn debug_output_hides_the_private_key() {
        let identity = StoredIdentity {
            id: "1".into(),
            created_at: "2026-01-01T00:00:00.000Z".into(),
            nick: "Sam".into(),
            pubkey: "ab".into(),
            privkey: Some(HEX.into()),
            color: None,
            bunker: None,
            custody: Custody::Shared,
            custody_at: None,
            browsers: None,
            deleted_at: None,
        };
        assert!(!format!("{identity:?}").contains(HEX));
    }
}
