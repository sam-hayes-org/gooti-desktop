//! Checks the Rust backup code against values the extension's own TypeScript produced
//! (`tests/vectors/extension.json`, from `npm run vectors`). Also writes `target/vectors/rust.json`
//! for the other direction (`npm run vectors:check`).

use std::path::PathBuf;

use gooti_core::backup::blob::{open_shared_document, seal_shared_document, shared_content_chars};
use gooti_core::backup::canonical::canonical_shared_document;
use gooti_core::backup::changes::describe_changes;
use gooti_core::backup::document::{SharedDocument, parse_shared_document};
use gooti_core::backup::event::{SHARED_D_TAG, build_backup_event, read_backup_payload};
use gooti_core::backup::merge::{active_shared_document, merge_shared_documents};
use gooti_core::backup::phrase::{derive_shared_keys, is_passphrase, normalize_passphrase};
use nostr::event::Event;
use serde_json::Value;

fn vectors() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/extension.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn doc(value: &Value) -> SharedDocument {
    parse_shared_document(value.clone()).expect("the extension's document is valid")
}

fn same(a: &SharedDocument, b: &SharedDocument) -> bool {
    canonical_shared_document(a) == canonical_shared_document(b)
}

#[test]
fn passphrases_give_the_same_keys() {
    let v = vectors();
    for case in v["keys"].as_array().unwrap() {
        let phrase = case["phrase"].as_str().unwrap();
        assert_eq!(normalize_passphrase(phrase).as_str(), case["normalized"]);
        let keys = derive_shared_keys(phrase).unwrap();
        assert_eq!(keys.signing_hex().as_str(), case["signingKey"], "{phrase}");
        assert_eq!(
            keys.encryption_hex().as_str(),
            case["encryptionKey"],
            "{phrase}"
        );
        assert_eq!(keys.pubkey_hex(), case["pubkey"], "{phrase}");
    }
    for case in v["invalidPhrases"].as_array().unwrap() {
        let phrase = case["phrase"].as_str().unwrap();
        assert_eq!(
            is_passphrase(phrase),
            case["valid"].as_bool().unwrap(),
            "{phrase}"
        );
        assert!(derive_shared_keys(phrase).is_err());
    }
}

#[test]
fn opens_the_extension_blob_and_event() {
    let v = vectors();
    let blob = &v["blob"];
    let keys = derive_shared_keys(blob["phrase"].as_str().unwrap()).unwrap();
    let expected = doc(&blob["document"]);

    let opened = open_shared_document(blob["payload"].as_str().unwrap(), &keys.encryption_key)
        .unwrap()
        .expect("a v2 document");
    assert_eq!(opened, expected);
    assert_eq!(
        shared_content_chars(&expected),
        blob["contentChars"].as_u64().unwrap() as usize
    );

    // An older document opens as "nothing", so it gets replaced.
    assert!(
        open_shared_document(blob["oldPayload"].as_str().unwrap(), &keys.encryption_key)
            .unwrap()
            .is_none()
    );

    let event: Event = serde_json::from_value(blob["event"].clone()).unwrap();
    assert_eq!(
        read_backup_payload(&event, SHARED_D_TAG).unwrap(),
        blob["payload"]
    );
    assert_eq!(event.pubkey.to_hex(), keys.pubkey_hex());

    // The same content and time give the same event id (the signature is randomized).
    let rebuilt = build_backup_event(
        &keys,
        SHARED_D_TAG,
        &event.content,
        event.created_at.as_secs(),
    );
    assert_eq!(rebuilt.id, event.id);

    // A changed payload or a wrong key fails.
    let other = derive_shared_keys(v["keys"][1]["phrase"].as_str().unwrap()).unwrap();
    assert!(
        open_shared_document(blob["payload"].as_str().unwrap(), &other.encryption_key).is_err()
    );
    let mut edited = event.clone();
    edited.content.push('A');
    assert!(read_backup_payload(&edited, SHARED_D_TAG).is_err());
}

#[test]
fn merges_like_the_extension() {
    let v = vectors();
    for case in v["merges"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let local = doc(&case["local"]);
        let remote = (!case["remote"].is_null()).then(|| doc(&case["remote"]));
        let merged = match &remote {
            Some(remote) => merge_shared_documents(&local, remote),
            None => local.clone(),
        };
        let expected = doc(&case["merged"]);
        assert_eq!(merged, expected, "{name}: merged");
        assert!(
            same(&active_shared_document(&merged), &doc(&case["active"])),
            "{name}: active"
        );

        let published = remote.as_ref().is_none_or(|remote| !same(&merged, remote));
        let changed = !same(&merged, &local);
        assert_eq!(
            published,
            case["published"].as_bool().unwrap(),
            "{name}: published"
        );
        assert_eq!(
            changed,
            case["changed"].as_bool().unwrap(),
            "{name}: changed"
        );

        let report = describe_changes(&local, remote.as_ref(), &merged, published);
        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            case["report"],
            "{name}: report"
        );
    }
}

#[test]
fn validates_documents_like_the_extension() {
    let v = vectors();
    for case in v["documents"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let parsed = parse_shared_document(case["value"].clone());
        assert_eq!(parsed.is_some(), case["valid"].as_bool().unwrap(), "{name}");
        if let Some(parsed) = parsed {
            let mut expected = case["normalized"].clone();
            // Only the five document fields travel; the extension's object may hold more.
            let object = expected.as_object_mut().unwrap();
            object.retain(|key, _| {
                ["v", "identities", "permissions", "relays", "sites"].contains(&key.as_str())
            });
            assert_eq!(serde_json::to_value(&parsed).unwrap(), expected, "{name}");
        }
    }
}

#[test]
fn colors_match_the_extension() {
    let v = vectors();
    for case in v["colors"].as_array().unwrap() {
        let pubkey = case["pubkey"].as_str().unwrap();
        assert_eq!(
            gooti_core::color_for_pubkey(pubkey, None),
            case["color"],
            "{pubkey}"
        );
    }
}

/// Seals the extension's document with Rust for `npm run vectors:check`.
#[test]
fn writes_a_rust_sealed_vault_for_the_extension_to_open() {
    let v = vectors();
    let phrase = v["keys"][2]["phrase"].as_str().unwrap();
    let keys = derive_shared_keys(phrase).unwrap();
    let document = doc(&v["blob"]["document"]);
    let payload = seal_shared_document(&document, &keys.encryption_key).unwrap();
    let event = build_backup_event(&keys, SHARED_D_TAG, &payload, 1_762_000_123);
    assert_eq!(
        open_shared_document(&payload, &keys.encryption_key)
            .unwrap()
            .unwrap(),
        document
    );

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/vectors");
    std::fs::create_dir_all(&dir).unwrap();
    let out = serde_json::json!({
        "phrase": phrase,
        "pubkey": keys.pubkey_hex(),
        "event": event,
        "document": document,
    });
    std::fs::write(
        dir.join("rust.json"),
        serde_json::to_string_pretty(&out).unwrap(),
    )
    .unwrap();
}
