//! Combines two vault snapshots. Port of `backup/merge.ts`.
//!
//! A higher revision wins (the later of `updatedAt` and `deletedAt`). A record on only one side
//! is kept, tombstones included. A tie keeps the local record. Order: local rows first, then rows
//! only the remote has, like the JavaScript `Map`.

use std::collections::HashMap;

use super::document::{Revisioned, SharedDocument, SharedIdentity};

pub fn merge_shared_documents(local: &SharedDocument, remote: &SharedDocument) -> SharedDocument {
    SharedDocument {
        v: 2,
        identities: merge_by_key(
            &local.identities,
            &remote.identities,
            |identity| identity.pubkey.clone(),
            prefer_identity,
        ),
        permissions: merge_by_key(&local.permissions, &remote.permissions, |p| p.key(), prefer),
        relays: Vec::new(),
        sites: merge_by_key(
            &local.sites,
            &remote.sites,
            |site| site.host.clone(),
            prefer,
        ),
    }
}

/// The records a device keeps. Tombstones stay in the merged document.
pub fn active_shared_document(document: &SharedDocument) -> SharedDocument {
    SharedDocument {
        v: 2,
        identities: document
            .identities
            .iter()
            .filter(|r| r.is_active())
            .cloned()
            .collect(),
        permissions: document
            .permissions
            .iter()
            .filter(|r| r.is_active())
            .cloned()
            .collect(),
        relays: Vec::new(),
        sites: document
            .sites
            .iter()
            .filter(|r| r.is_active())
            .cloned()
            .collect(),
    }
}

fn merge_by_key<T: Clone>(
    local: &[T],
    remote: &[T],
    key_of: impl Fn(&T) -> String,
    choose: impl Fn(&T, &T) -> T,
) -> Vec<T> {
    let mut merged: Vec<T> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for record in local {
        let key = key_of(record);
        match index.get(&key) {
            // A later duplicate replaces the earlier one, like `Map.set`.
            Some(&at) => merged[at] = record.clone(),
            None => {
                index.insert(key, merged.len());
                merged.push(record.clone());
            }
        }
    }
    for record in remote {
        let key = key_of(record);
        match index.get(&key) {
            Some(&at) => merged[at] = choose(&merged[at], record),
            None => {
                index.insert(key, merged.len());
                merged.push(record.clone());
            }
        }
    }
    merged
}

fn prefer<T: Revisioned + Clone>(local: &T, remote: &T) -> T {
    if remote.revision() > local.revision() {
        remote.clone()
    } else {
        local.clone()
    }
}

/// A key against a bunker pointer: the later `custodyAt` wins (absent counts as 0), so a device
/// that still has the key cannot undo a move to Gooti Desktop by editing its copy.
/// Otherwise a higher revision wins. A tie keeps the local row, and fills a conversation key
/// the local row does not have.
fn prefer_identity(local: &SharedIdentity, remote: &SharedIdentity) -> SharedIdentity {
    // A tombstone without a key or a pointer (a newer Gooti keeps no secrets in it) has no custody:
    // its revision decides, like any other deletion.
    let keyless = |row: &SharedIdentity| row.privkey.is_none() && row.bunker.is_none();
    if local.privkey.is_some() != remote.privkey.is_some() && !keyless(local) && !keyless(remote) {
        let local_custody = local.custody_at.unwrap_or(0.0);
        let remote_custody = remote.custody_at.unwrap_or(0.0);
        if local_custody != remote_custody {
            return if remote_custody > local_custody {
                remote.clone()
            } else {
                local.clone()
            };
        }
    }
    if remote.revision() != local.revision() {
        return prefer(local, remote);
    }
    let (Some(bunker), Some(remote_bunker)) = (&local.bunker, &remote.bunker) else {
        return local.clone();
    };
    let Some(spare) = &remote_bunker.client_secret_key else {
        return local.clone();
    };
    if bunker.client_secret_key.is_some()
        || bunker.pubkey != remote_bunker.pubkey
        || bunker.secret != remote_bunker.secret
    {
        return local.clone();
    }
    let mut filled = local.clone();
    if let Some(bunker) = filled.bunker.as_mut() {
        bunker.client_secret_key = Some(spare.clone());
    }
    filled
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::backup::document::parse_shared_document;

    const PRIVKEY: &str = "abababababababababababababababababababababababababababababababab";

    fn pubkey() -> String {
        crate::identity::pubkey_for_privkey(PRIVKEY).unwrap()
    }

    fn document(row: serde_json::Value) -> serde_json::Value {
        json!({ "v": 2, "identities": [row], "permissions": [], "relays": [], "sites": [] })
    }

    #[test]
    fn a_tombstone_without_key_or_pointer_is_valid_and_a_live_row_still_needs_one() {
        let tombstone = json!({
            "pubkey": pubkey(), "nick": "Ada", "createdAt": "2026-01-01T00:00:00.000Z",
            "updatedAt": 5, "deletedAt": 8
        });
        assert!(parse_shared_document(document(tombstone.clone())).is_some());

        let mut live = tombstone.clone();
        live.as_object_mut().unwrap().remove("deletedAt");
        assert!(parse_shared_document(document(live)).is_none());

        let mut both = tombstone;
        both["privkey"] = json!(PRIVKEY);
        both["bunker"] = json!({ "pubkey": pubkey(), "relays": ["wss://r"], "secret": null });
        assert!(parse_shared_document(document(both)).is_none());
    }

    #[test]
    fn a_keyless_tombstone_beats_an_older_live_row_whatever_its_custody() {
        let mut live = parse_shared_document(document(json!({
            "pubkey": pubkey(), "privkey": PRIVKEY, "nick": "Ada",
            "createdAt": "2026-01-01T00:00:00.000Z", "updatedAt": 5, "custodyAt": 100
        })))
        .unwrap();
        let dead = parse_shared_document(document(json!({
            "pubkey": pubkey(), "nick": "Ada", "createdAt": "2026-01-01T00:00:00.000Z",
            "updatedAt": 5, "deletedAt": 8
        })))
        .unwrap();

        let merged = merge_shared_documents(&live, &dead);
        assert_eq!(merged.identities[0].deleted_at, Some(8.0));
        let merged = merge_shared_documents(&dead, &live);
        assert_eq!(merged.identities[0].deleted_at, Some(8.0));

        live.identities[0].updated_at = 20.0;
        let merged = merge_shared_documents(&dead, &live);
        assert!(merged.identities[0].deleted_at.is_none());
    }
}
