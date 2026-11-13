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

    fn keep_identities_where(&mut self, keep: impl Fn(&str) -> bool) {
        self.signers.retain(|s| keep(&s.identity_pubkey));
        self.invites.retain(|i| keep(&i.identity_pubkey));
        self.connections.retain(|c| keep(&c.identity_pubkey));
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
    /// The connection every browser uses for a Desktop only identity.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub browsers: bool,
}

/// The name of the browsers' connection.
pub const BROWSERS_NAME: &str = "Gooti browsers";

/// Name, URL and icon an app sends with `connect`. Display hints only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientMeta {
    pub name: Option<String>,
    pub url: Option<String>,
    pub image: Option<String>,
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
        }
    }
}
