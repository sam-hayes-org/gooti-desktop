//! The desktop record: what only Gooti Desktops share, next to the shared record that browsers
//! read too. It holds the private keys this desktop holds (Desktop only keys among them), the
//! bunker signer keys, the bunker connections with their trust and rules, and the bunker relays.
//!
//! Same passphrase, other keys (`derive_desktop_keys`). Browsers keep only the shared keys, so
//! they can neither read nor replace this record. On the relays it is kind 30078 with
//! `d` = `gooti-desktop`, from its own signing key.
//!
//! Every row carries the time of its last change. Merging keeps the later row; a tie keeps the
//! local one. A deleted row stays as a tombstone (no value), so a deletion travels too.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::blob::{open_plaintext, seal_plaintext};
use crate::bunker::state::Connection;
use crate::error::{Error, Result};

/// Milliseconds since 1970, like the shared record's stamps.
pub type Millis = f64;

/// The plaintext's marker and version.
const MARKER: &str = "gootiDesktop";
const VERSION: u64 = 1;

/// One row: its value, or none for a deleted row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", bound(deserialize = "T: Deserialize<'de>"))]
pub struct Row<T> {
    pub at: Millis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<T>,
}

impl<T> Row<T> {
    pub fn live(&self) -> Option<&T> {
        self.value.as_ref()
    }
}

/// A bunker signer key and the desktop that answers on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignerRow {
    pub identity_pubkey: String,
    pub secret_hex: String,
    /// The id of the one desktop that listens with this key.
    pub served_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopDocument {
    /// Identity pubkey → hex private key.
    #[serde(default)]
    pub keys: BTreeMap<String, Row<String>>,
    /// Signer pubkey → signer.
    #[serde(default)]
    pub signers: BTreeMap<String, Row<SignerRow>>,
    /// Connection id → connection. Its last-used time stays on each desktop.
    #[serde(default)]
    pub connections: BTreeMap<String, Row<Connection>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bunker_relays: Option<Row<Vec<String>>>,
}

impl DesktopDocument {
    pub fn signer(&self, signer_pubkey: &str) -> Option<&SignerRow> {
        self.signers.get(signer_pubkey).and_then(Row::live)
    }
}

/// Sets a live row, later than the row it replaces. False when the row already holds `value`.
pub fn put<T: PartialEq>(
    rows: &mut BTreeMap<String, Row<T>>,
    key: &str,
    value: T,
    now: Millis,
) -> bool {
    let previous = rows.get(key);
    if previous.and_then(Row::live) == Some(&value) {
        return false;
    }
    let at = previous.map_or(now, |p| now.max(p.at + 1.0));
    rows.insert(
        key.to_string(),
        Row {
            at,
            value: Some(value),
        },
    );
    true
}

/// Turns a live row into a tombstone. False when there is no live row.
pub fn bury<T>(rows: &mut BTreeMap<String, Row<T>>, key: &str, now: Millis) -> bool {
    let Some(previous) = rows.get(key).filter(|row| row.value.is_some()) else {
        return false;
    };
    let at = now.max(previous.at + 1.0);
    rows.insert(key.to_string(), Row { at, value: None });
    true
}

/// Row by row, the later one wins; a tie keeps `local`.
pub fn merge_desktop_documents(
    local: &DesktopDocument,
    remote: &DesktopDocument,
) -> DesktopDocument {
    DesktopDocument {
        keys: merge_rows(&local.keys, &remote.keys),
        signers: merge_rows(&local.signers, &remote.signers),
        connections: merge_rows(&local.connections, &remote.connections),
        bunker_relays: match (&local.bunker_relays, &remote.bunker_relays) {
            (Some(l), Some(r)) => Some(if r.at > l.at { r.clone() } else { l.clone() }),
            (l, r) => l.clone().or_else(|| r.clone()),
        },
    }
}

fn merge_rows<T: Clone>(
    local: &BTreeMap<String, Row<T>>,
    remote: &BTreeMap<String, Row<T>>,
) -> BTreeMap<String, Row<T>> {
    let mut merged = local.clone();
    for (key, theirs) in remote {
        match merged.get(key) {
            Some(ours) if ours.at >= theirs.at => {}
            _ => {
                merged.insert(key.clone(), theirs.clone());
            }
        }
    }
    merged
}

pub fn seal_desktop_document(
    document: &DesktopDocument,
    encryption_key: &[u8; 32],
) -> Result<String> {
    let mut value = serde_json::to_value(document).expect("a desktop document serializes");
    value
        .as_object_mut()
        .expect("a desktop document is an object")
        .insert(MARKER.into(), json!(VERSION));
    let plaintext = Zeroizing::new(serde_json::to_vec(&value).expect("JSON serializes"));
    seal_plaintext(&plaintext, encryption_key)
}

/// `Err` when the key is wrong, the blob was changed, or it is no desktop record.
pub fn open_desktop_document(payload: &str, encryption_key: &[u8; 32]) -> Result<DesktopDocument> {
    let plaintext = open_plaintext(payload, encryption_key)?;
    let value: Value = serde_json::from_slice(&plaintext).map_err(|_| Error::InvalidBackup)?;
    if value.get(MARKER) != Some(&json!(VERSION)) {
        return Err(Error::InvalidBackup);
    }
    serde_json::from_value(value).map_err(|_| Error::InvalidBackup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::blob::{open_shared_document, seal_shared_document};
    use crate::backup::document::SharedDocument;
    use crate::backup::phrase::{derive_desktop_keys, derive_shared_keys, generate_passphrase};

    fn signer(served_by: &str) -> SignerRow {
        SignerRow {
            identity_pubkey: "a".repeat(64),
            secret_hex: "1".repeat(64),
            served_by: served_by.into(),
        }
    }

    #[test]
    fn seals_and_opens_only_with_the_desktop_keys() {
        let phrase = generate_passphrase();
        let desktop = derive_desktop_keys(&phrase).unwrap();
        let shared = derive_shared_keys(&phrase).unwrap();
        let mut document = DesktopDocument::default();
        put(&mut document.keys, &"a".repeat(64), "2".repeat(64), 10.0);
        put(&mut document.signers, "s1", signer("d1"), 10.0);
        let payload = seal_desktop_document(&document, &desktop.encryption_key).unwrap();

        assert_eq!(
            open_desktop_document(&payload, &desktop.encryption_key).unwrap(),
            document
        );
        // A browser holds only the shared keys: they open nothing here.
        assert!(open_desktop_document(&payload, &shared.encryption_key).is_err());
        assert!(open_shared_document(&payload, &shared.encryption_key).is_err());
        // And a shared record is no desktop record, even under the right key.
        let shared_payload =
            seal_shared_document(&SharedDocument::default(), &desktop.encryption_key).unwrap();
        assert!(open_desktop_document(&shared_payload, &desktop.encryption_key).is_err());
    }

    #[test]
    fn rows_are_stamped_later_and_the_later_row_wins() {
        let mut local = DesktopDocument::default();
        assert!(put(&mut local.signers, "s1", signer("d1"), 100.0));
        assert!(!put(&mut local.signers, "s1", signer("d1"), 200.0));
        // A clock that runs behind still makes a later row.
        assert!(put(&mut local.signers, "s1", signer("d2"), 50.0));
        assert_eq!(local.signers["s1"].at, 101.0);

        let mut remote = DesktopDocument::default();
        put(&mut remote.signers, "s1", signer("d3"), 300.0);
        put(&mut remote.keys, "k", "x".into(), 5.0);
        let merged = merge_desktop_documents(&local, &remote);
        assert_eq!(merged.signer("s1").unwrap().served_by, "d3");
        assert_eq!(merged.keys["k"].live().unwrap(), "x");

        // A tombstone travels, and a tie keeps the local row.
        let mut buried = merged.clone();
        assert!(bury(&mut buried.signers, "s1", 0.0));
        assert!(!bury(&mut buried.signers, "s1", 0.0));
        let back = merge_desktop_documents(&merged, &buried);
        assert!(back.signer("s1").is_none());
        let tie = merge_desktop_documents(&merged, &merged.clone());
        assert_eq!(tie, merged);
    }
}
