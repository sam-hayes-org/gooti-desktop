//! What the bunker keeps in the vault: its relays, signer keys, open `bunker://` invitations, and
//! the apps connected to it. All but the invitations and the pause also go into the desktop
//! record (`backup::desktop`), which every Gooti Desktop with the passphrase shares.

use serde::{Deserialize, Serialize};

use super::policy::{Rule, Trust};
use zeroize::Zeroize;

/// Gooti's relay only. Users can add more under Connections → Relays.
pub const DEFAULT_BUNKER_RELAYS: [&str; 1] = ["wss://relay.getgooti.com"];

/// A `bunker://` secret works once, within this time.
pub const INVITE_LIFETIME_MS: i64 = 60 * 60 * 1000;

fn default_relays() -> Vec<String> {
    DEFAULT_BUNKER_RELAYS
        .iter()
        .map(|r| r.to_string())
        .collect()
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BunkerData {
    /// The default relays for apps. They fill new links and new Desktop only identities; what is
    /// in use keeps its own copy, so a change here changes nothing that exists.
    #[serde(default = "default_relays")]
    pub relays: Vec<String>,
    /// Stops answering every app at once.
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub signers: Vec<SignerKey>,
    #[serde(default)]
    pub invites: Vec<Invite>,
    #[serde(default)]
    pub connections: Vec<Connection>,
    /// When the user last set `relays` (ms). `None`: the defaults, which the desktop record
    /// never overrides another desktop's choice with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relays_at: Option<f64>,
    /// Connections ended here (revoked, logged out) since the desktop record last saw them.
    /// Its next backup deletes them there too.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
    /// Browsers with their own key (`docs/DESIGN-browser-keys.md`): approved or blocked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub browsers: Vec<BrowserDevice>,
    /// The shared browser key of Gooti 2.0 browsers, retired: its requests are told to update
    /// Gooti. Browsers now sign with their own key (`docs/DESIGN-browser-keys.md`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retired_shared_keys: Vec<String>,
    /// Identity → the relays browsers reach that Desktop only identity on. Its pointer names
    /// them, and each browser's connection uses them. A change of the default relays leaves them.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub browser_relays: std::collections::BTreeMap<String, Vec<String>>,
    /// Identity → how far browsers may go for that Desktop only identity without asking. The same
    /// for every browser; missing is Basic.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub browser_trust: std::collections::BTreeMap<String, Trust>,
}

/// A browser that asked to sign through this desktop with its own key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDevice {
    /// The browser's own key; the same for all identities.
    pub client_pubkey: String,
    pub name: String,
    /// When Allow was pressed (ms). `None` while blocked without ever being allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<i64>,
    /// Denied or disconnected: its requests fail and it cannot ask again, for
    /// `BLOCKED_KEPT_MS`. After that it is forgotten; a key that asks again is asked about again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub blocked: bool,
    /// When it was blocked (ms). Set at unlock for a block from before 2026-11-19.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_at: Option<i64>,
}

/// How long a denied or disconnected browser key stays blocked: long enough that a lost
/// browser is gone for good, short enough that the list does not grow forever.
pub const BLOCKED_KEPT_MS: i64 = 90 * 24 * 60 * 60 * 1000;

impl BrowserDevice {
    /// Blocked long enough ago to be forgotten.
    pub fn block_expired(&self, now: i64) -> bool {
        self.blocked
            && self
                .blocked_at
                .is_some_and(|at| now - at >= BLOCKED_KEPT_MS)
    }
}

/// What `connect` metadata says when a Gooti browser asks with its own key.
pub const BROWSER_HINT: &str = "browser";

/// The secret a browser sends with its `connect`, from the pointer in the backup. Derived from the
/// identity's private key, so every Gooti Desktop that holds the key expects the same one, and
/// only someone who can open the backup (the pointer is sealed inside it) knows it. Without it, anyone
/// who sees the signer key on a relay could make this desktop ask "Allow this browser?".
pub fn browser_secret(privkey_hex: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"gooti browser connect\0");
    hasher.update(privkey_hex.trim().to_ascii_lowercase().as_bytes());
    crate::backup::phrase::hex(&hasher.finalize()[..16])
}

/// "K7F-2QX": the first 30 bits of SHA-256 over the 32-byte key, in Crockford base32. The
/// extension shows the same code for its own key, so a lost browser can be told from the others.
pub fn browser_code(client_pubkey_hex: &str) -> String {
    use sha2::{Digest, Sha256};
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let bytes: Vec<u8> = (0..client_pubkey_hex.len() / 2)
        .filter_map(|i| u8::from_str_radix(client_pubkey_hex.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect();
    let hash = Sha256::digest(&bytes);
    let bits = u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]) >> 2;
    let chars: String = (0..6)
        .map(|i| ALPHABET[((bits >> (25 - i * 5)) & 31) as usize] as char)
        .collect();
    format!("{}-{}", &chars[..3], &chars[3..])
}

impl Default for BunkerData {
    fn default() -> Self {
        BunkerData {
            relays: default_relays(),
            paused: false,
            signers: Vec::new(),
            invites: Vec::new(),
            connections: Vec::new(),
            relays_at: None,
            removed: Vec::new(),
            browsers: Vec::new(),
            retired_shared_keys: Vec::new(),
            browser_relays: Default::default(),
            browser_trust: Default::default(),
        }
    }
}

impl BunkerData {
    /// Gives every app, Gooti browsers too, relays of its own. Before 2026-11-12, apps from a
    /// `bunker://` link and the browsers kept none and followed the default relays: they get
    /// today's, so a later change of the default leaves them working. Returns whether anything changed.
    pub fn pin_app_relays(&mut self) -> bool {
        let mut changed = false;
        for connection in &mut self.connections {
            if connection.relays.is_empty() && !self.relays.is_empty() {
                connection.relays = self.relays.clone();
                changed = true;
            }
        }
        changed
    }

    /// Drops the signer key, links and connections of a deleted identity.
    pub fn forget_identity(&mut self, identity_pubkey: &str) {
        self.keep_identities_where(|pubkey| pubkey != identity_pubkey);
    }

    /// Keeps only what belongs to these identities.
    pub fn keep_identities(&mut self, identity_pubkeys: &[String]) {
        self.keep_identities_where(|pubkey| identity_pubkeys.iter().any(|kept| kept == pubkey));
    }

    /// Ends the connections of the shared browser key (Gooti 2.0 browsers, or one that came from
    /// an older Gooti Desktop) and remembers the key, so its requests are told to update Gooti.
    /// Returns whether anything changed.
    pub fn retire_shared_key(&mut self) -> bool {
        let shared: Vec<(String, String)> = self
            .connections
            .iter()
            .filter(|c| c.browsers && self.browser(&c.client_pubkey).is_none())
            .map(|c| (c.id.clone(), c.client_pubkey.clone()))
            .collect();
        if shared.is_empty() {
            return false;
        }
        // The shared connection held the identity's browser relays and trust until 2026-11-17:
        // keep them.
        for connection in self.connections.iter().filter(|c| c.browsers) {
            if !connection.relays.is_empty() {
                self.browser_relays
                    .entry(connection.identity_pubkey.clone())
                    .or_insert_with(|| connection.relays.clone());
            }
            if self.browser(&connection.client_pubkey).is_none() {
                self.browser_trust
                    .entry(connection.identity_pubkey.clone())
                    .or_insert(connection.trust);
            }
        }
        self.connections
            .retain(|c| !shared.iter().any(|(id, _)| *id == c.id));
        for (id, client) in shared {
            self.removed.push(id);
            if !self.retired_shared_keys.contains(&client) {
                self.retired_shared_keys.push(client);
            }
        }
        true
    }

    pub fn browser(&self, client_pubkey: &str) -> Option<&BrowserDevice> {
        self.browsers
            .iter()
            .find(|b| b.client_pubkey == client_pubkey)
    }

    fn keep_identities_where(&mut self, keep: impl Fn(&str) -> bool) {
        self.signers.retain(|s| keep(&s.identity_pubkey));
        self.invites.retain(|i| keep(&i.identity_pubkey));
        self.connections.retain(|c| keep(&c.identity_pubkey));
        self.browser_relays.retain(|pubkey, _| keep(pubkey));
        self.browser_trust.retain(|pubkey, _| keep(pubkey));
    }

    /// Dates blocks from before 2026-11-19, and forgets those older than `BLOCKED_KEPT_MS`.
    /// Returns whether anything changed.
    pub fn tidy_blocked_browsers(&mut self, now: i64) -> bool {
        let mut changed = false;
        for browser in self.browsers.iter_mut().filter(|b| b.blocked) {
            if browser.blocked_at.is_none() {
                browser.blocked_at = Some(now);
                changed = true;
            }
        }
        let before = self.browsers.len();
        self.browsers.retain(|b| !b.block_expired(now));
        changed || self.browsers.len() != before
    }

    /// How far browsers may go for this Desktop only identity without asking.
    pub fn browser_trust_of(&self, identity_pubkey: &str) -> Trust {
        self.browser_trust
            .get(identity_pubkey)
            .copied()
            .unwrap_or(Trust::Basic)
    }
}

/// A key apps talk to for one identity. Not the identity's own key, so relays do not see
/// which identity an app uses. An identity can have more than one: each Gooti Desktop that
/// started serving it made its own.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignerKey {
    pub identity_pubkey: String,
    pub secret_hex: String,
    /// The desktop that listens with this key. `None` is this desktop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub served_by: Option<String>,
}

impl SignerKey {
    pub fn served_by(&self, desktop_id: &str) -> bool {
        self.served_by.as_deref().is_none_or(|by| by == desktop_id)
    }
}

impl Drop for SignerKey {
    fn drop(&mut self) {
        self.secret_hex.zeroize();
    }
}

/// An open `bunker://` link.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    pub secret: String,
    pub identity_pubkey: String,
    pub created_at: i64,
    /// The relays the link names. Empty in links made before 2026-11-12: the default relays.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relays: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: String,
    pub identity_pubkey: String,
    pub client_pubkey: String,
    /// Where this app listens. Empty for apps that came with a `bunker://` link: they use
    /// the bunker's relays.
    #[serde(default)]
    pub relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// What the app asked for, such as `sign_event:1,nip44_encrypt`. Shown, not enforced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perms: Option<String>,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<i64>,
    #[serde(default)]
    pub trust: Trust,
    /// Choices the user remembered for this app.
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// A browser's connection (its own key) for a Desktop only identity.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub browsers: bool,
}

/// Name, URL and icon an app sends with `connect`. Display hints only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientMeta {
    pub name: Option<String>,
    pub url: Option<String>,
    pub image: Option<String>,
    /// `"browser"`: a Gooti browser asking with its own key.
    pub gooti: Option<String>,
}

impl ClientMeta {
    pub fn from_json(text: &str) -> ClientMeta {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            return ClientMeta::default();
        };
        let field = |key: &str| {
            value
                .get(key)
                .and_then(|v| v.as_str())
                .map(|s| s.chars().take(200).collect::<String>())
                .filter(|s| !s.trim().is_empty())
        };
        ClientMeta {
            name: field("name"),
            url: field("url"),
            image: field("image"),
            gooti: field("gooti"),
        }
    }
}
