//! The plaintext inside one backup blob. Port of `gooti-extension/.../backup/document.ts`.
//!
//! Field names, optional fields and validation match the extension. Unknown fields on a row are
//! kept, so a newer extension's data survives a round trip through this desktop.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::identity::{normalize_color_loose, pubkey_for_privkey};

/// NIP-07 methods a permission row may name. Same list as the extension's `NIP07_METHODS`.
pub const NIP07_METHODS: [&str; 6] = [
    "signEvent",
    "getPublicKey",
    "nip04.encrypt",
    "nip04.decrypt",
    "nip44.encrypt",
    "nip44.decrypt",
];

/// JavaScript numbers. Milliseconds are whole in practice, but the extension accepts any
/// finite number, so a fraction must survive too.
pub type Millis = f64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SharedDocument {
    pub v: u8,
    pub identities: Vec<SharedIdentity>,
    pub permissions: Vec<SharedPermission>,
    /// Always empty. Identity publish relays are no longer stored.
    pub relays: Vec<Value>,
    #[serde(default)]
    pub sites: Vec<SharedSite>,
}

impl Default for SharedDocument {
    fn default() -> Self {
        SharedDocument {
            v: 2,
            identities: Vec::new(),
            permissions: Vec::new(),
            relays: Vec::new(),
            sites: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedIdentity {
    pub pubkey: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privkey: Option<String>,
    pub nick: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub created_at: String,
    #[serde(with = "js_number")]
    pub updated_at: Millis,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "js_number_opt"
    )]
    pub deleted_at: Option<Millis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bunker: Option<SharedBunker>,
    /// When the private key last moved to a Gooti Desktop or back to the browsers. When one row holds
    /// the key and the other is a bunker pointer, the later `custodyAt` wins (see `merge.rs`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "js_number_opt"
    )]
    pub custody_at: Option<Millis>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Relays and secret from a `bunker://` link.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedBunker {
    pub pubkey: String,
    pub relays: Vec<String>,
    /// Always written, `null` when the link had none. The extension rejects a missing secret.
    pub secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask_site: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret_key: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedPermission {
    pub identity_pubkey: String,
    pub host: String,
    pub method: String,
    pub method_policy: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "js_number_opt"
    )]
    pub kind: Option<f64>,
    #[serde(with = "js_number")]
    pub updated_at: Millis,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "js_number_opt"
    )]
    pub deleted_at: Option<Millis>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedSite {
    pub host: String,
    pub identity_pubkey: String,
    #[serde(with = "js_number")]
    pub updated_at: Millis,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "js_number_opt"
    )]
    pub deleted_at: Option<Millis>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Rows that carry a revision: `updatedAt` and maybe `deletedAt`.
pub trait Revisioned {
    fn updated_at(&self) -> Millis;
    fn deleted_at(&self) -> Option<Millis>;

    fn revision(&self) -> Millis {
        self.updated_at().max(self.deleted_at().unwrap_or(0.0))
    }

    fn is_active(&self) -> bool {
        self.deleted_at().is_none()
    }
}

macro_rules! revisioned {
    ($($row:ty),*) => {$(
        impl Revisioned for $row {
            fn updated_at(&self) -> Millis { self.updated_at }
            fn deleted_at(&self) -> Option<Millis> { self.deleted_at }
        }
    )*};
}
revisioned!(SharedIdentity, SharedPermission, SharedSite);

impl SharedPermission {
    /// Same key as the extension's `permissionKey`.
    pub fn key(&self) -> String {
        let kind = self.kind.map(js_number_text).unwrap_or_default();
        format!(
            "{}\n{}\n{}\n{}",
            self.identity_pubkey, self.host, self.method, kind
        )
    }
}

/// `${n}` in JavaScript for the numbers a document holds.
pub fn js_number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// `bunkerAsksSite`: a missing choice asks which site.
pub fn bunker_asks_site(bunker: Option<&SharedBunker>) -> bool {
    bunker.and_then(|b| b.ask_site) != Some(false)
}

/// Checks and normalizes a parsed document like the extension's `assertSyncDocument`:
/// `relays` is emptied, `getRelays` permissions are dropped, a missing `sites` becomes empty.
pub fn parse_shared_document(mut value: Value) -> Option<SharedDocument> {
    let object = value.as_object_mut()?;
    if object.get("v") != Some(&Value::from(2)) {
        return None;
    }
    if !object.get("identities")?.is_array() || !object.get("permissions")?.is_array() {
        return None;
    }
    match object.get("relays") {
        None => {}
        Some(relays) if relays.is_array() => {}
        Some(_) => return None,
    }
    object.insert("relays".into(), Value::Array(Vec::new()));
    match object.get("sites") {
        None => {
            object.insert("sites".into(), Value::Array(Vec::new()));
        }
        Some(sites) if sites.is_array() => {}
        Some(_) => return None,
    }
    if let Some(Value::Array(permissions)) = object.get_mut("permissions") {
        permissions.retain(|item| item.get("method") != Some(&Value::from("getRelays")));
    }

    let rows_valid = object["identities"].as_array()?.iter().all(valid_identity)
        && object["permissions"]
            .as_array()?
            .iter()
            .all(valid_permission)
        && object["sites"].as_array()?.iter().all(valid_site);
    if !rows_valid {
        return None;
    }
    // Only the five known fields travel in a sealed blob.
    let document = serde_json::json!({
        "v": 2,
        "identities": object["identities"],
        "permissions": object["permissions"],
        "relays": [],
        "sites": object["sites"],
    });
    serde_json::from_value(document).ok()
}

fn valid_identity(value: &Value) -> bool {
    let Some(row) = value.as_object() else {
        return false;
    };
    if !is_key(row.get("pubkey")) || !row.get("nick").is_some_and(Value::is_string) {
        return false;
    }
    if !row.get("createdAt").is_some_and(Value::is_string) || !is_time(row.get("updatedAt")) {
        return false;
    }
    if let Some(color) = row.get("color")
        && color.as_str().and_then(normalize_color_loose).is_none()
    {
        return false;
    }
    if !optional_time(row.get("deletedAt")) {
        return false;
    }
    let privkey = row.get("privkey");
    let bunker = row.get("bunker");
    if privkey.is_some() == bunker.is_some() {
        return false;
    }
    if let Some(privkey) = privkey {
        let pubkey = row["pubkey"].as_str().unwrap_or_default();
        let matches = privkey
            .as_str()
            .filter(|key| is_hex_key(key))
            .and_then(pubkey_for_privkey)
            .is_some_and(|derived| derived == pubkey);
        if !matches {
            return false;
        }
    }
    bunker.is_none_or(valid_bunker)
}

fn valid_bunker(value: &Value) -> bool {
    let Some(row) = value.as_object() else {
        return false;
    };
    if !is_key(row.get("pubkey")) {
        return false;
    }
    let Some(relays) = row.get("relays").and_then(Value::as_array) else {
        return false;
    };
    if relays.is_empty()
        || !relays
            .iter()
            .all(|relay| relay.as_str().is_some_and(is_wss_url))
    {
        return false;
    }
    match row.get("secret") {
        Some(Value::Null) | Some(Value::String(_)) => {}
        _ => return false,
    }
    if row.get("askSite").is_some_and(|v| !v.is_boolean()) {
        return false;
    }
    row.get("clientSecretKey")
        .is_none_or(|key| is_key(Some(key)))
}

fn valid_permission(value: &Value) -> bool {
    let Some(row) = value.as_object() else {
        return false;
    };
    if !is_key(row.get("identityPubkey")) || !row.get("host").is_some_and(Value::is_string) {
        return false;
    }
    let method_ok = row
        .get("method")
        .and_then(Value::as_str)
        .is_some_and(|method| NIP07_METHODS.contains(&method));
    let policy_ok = matches!(
        row.get("methodPolicy").and_then(Value::as_str),
        Some("allow" | "deny")
    );
    if !method_ok || !policy_ok {
        return false;
    }
    if row.get("kind").is_some_and(|kind| !kind.is_number()) {
        return false;
    }
    is_time(row.get("updatedAt")) && optional_time(row.get("deletedAt"))
}

fn valid_site(value: &Value) -> bool {
    let Some(row) = value.as_object() else {
        return false;
    };
    let host_ok = row.get("host").and_then(Value::as_str).is_some_and(|host| {
        !host.is_empty() && !host.chars().any(char::is_whitespace) && !host.contains('/')
    });
    host_ok
        && is_key(row.get("identityPubkey"))
        && is_time(row.get("updatedAt"))
        && optional_time(row.get("deletedAt"))
}

fn is_key(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str).is_some_and(is_hex_key)
}

/// 64 lower-case hex characters.
pub fn is_hex_key(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `wss://` followed by at least one non-space character, any case, like the extension's check.
pub fn is_wss_url(text: &str) -> bool {
    text.len() > 6
        && text[..6].eq_ignore_ascii_case("wss://")
        && !text[6..].chars().any(char::is_whitespace)
}

fn is_time(value: Option<&Value>) -> bool {
    value.is_some_and(Value::is_number)
}

fn optional_time(value: Option<&Value>) -> bool {
    value.is_none_or(Value::is_number)
}

/// Writes whole numbers without `.0`, so JavaScript and Rust print the same JSON.
mod js_number {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        if value.fract() == 0.0 && value.abs() < 9.007_199_254_740_992e15 {
            serializer.serialize_i64(*value as i64)
        } else {
            serializer.serialize_f64(*value)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        f64::deserialize(deserializer)
    }
}

mod js_number_opt {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => super::js_number::serialize(value, serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<f64>, D::Error> {
        Option::<f64>::deserialize(deserializer)
    }
}
