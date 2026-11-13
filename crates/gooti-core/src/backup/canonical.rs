//! Stable JSON so two equal vaults compare equal regardless of row order or key order.
//! Port of `canonicalSyncDocument`. Only used to compare documents, never published.

use serde_json::{Map, Value, json};

use super::document::{SharedBunker, SharedDocument};

pub fn canonical_shared_document(document: &SharedDocument) -> String {
    let mut identities: Vec<_> = document.identities.iter().collect();
    identities.sort_by(|a, b| a.pubkey.cmp(&b.pubkey));
    let identities: Vec<Value> = identities
        .into_iter()
        .map(|identity| {
            let mut row = Map::new();
            row.insert("pubkey".into(), json!(identity.pubkey));
            if let Some(privkey) = &identity.privkey {
                row.insert("privkey".into(), json!(privkey));
            }
            row.insert("nick".into(), json!(identity.nick));
            if let Some(color) = &identity.color {
                row.insert("color".into(), json!(color));
            }
            row.insert("createdAt".into(), json!(identity.created_at));
            row.insert("updatedAt".into(), json!(identity.updated_at));
            if let Some(deleted) = identity.deleted_at {
                row.insert("deletedAt".into(), json!(deleted));
            }
            if let Some(bunker) = &identity.bunker {
                row.insert("bunker".into(), published_bunker_value(bunker));
            }
            if let Some(custody_at) = identity.custody_at {
                row.insert("custodyAt".into(), json!(custody_at));
            }
            Value::Object(row)
        })
        .collect();

    let mut permissions: Vec<_> = document.permissions.iter().collect();
    permissions.sort_by_key(|p| p.key());
    let permissions: Vec<Value> = permissions
        .into_iter()
        .map(|p| {
            let mut row = Map::new();
            row.insert("identityPubkey".into(), json!(p.identity_pubkey));
            row.insert("host".into(), json!(p.host));
            row.insert("method".into(), json!(p.method));
            row.insert("methodPolicy".into(), json!(p.method_policy));
            if let Some(kind) = p.kind {
                row.insert("kind".into(), json!(kind));
            }
            row.insert("updatedAt".into(), json!(p.updated_at));
            if let Some(deleted) = p.deleted_at {
                row.insert("deletedAt".into(), json!(deleted));
            }
            Value::Object(row)
        })
        .collect();

    let mut sites: Vec<_> = document.sites.iter().collect();
    sites.sort_by(|a, b| a.host.cmp(&b.host));
    let sites: Vec<Value> = sites
        .into_iter()
        .map(|site| {
            let mut row = Map::new();
            row.insert("host".into(), json!(site.host));
            row.insert("identityPubkey".into(), json!(site.identity_pubkey));
            row.insert("updatedAt".into(), json!(site.updated_at));
            if let Some(deleted) = site.deleted_at {
                row.insert("deletedAt".into(), json!(deleted));
            }
            Value::Object(row)
        })
        .collect();

    json!({ "v": 2, "identities": identities, "permissions": permissions, "relays": [], "sites": sites })
        .to_string()
}

/// The bunker fields that travel: pubkey, relays, secret, askSite, a valid conversation key.
pub fn published_bunker(bunker: &SharedBunker) -> SharedBunker {
    SharedBunker {
        pubkey: bunker.pubkey.clone(),
        relays: bunker.relays.clone(),
        secret: bunker.secret.clone(),
        ask_site: bunker.ask_site,
        client_secret_key: bunker
            .client_secret_key
            .clone()
            .filter(|key| super::document::is_hex_key(key)),
        extra: Map::new(),
    }
}

fn published_bunker_value(bunker: &SharedBunker) -> Value {
    serde_json::to_value(published_bunker(bunker)).expect("a bunker serializes")
}
