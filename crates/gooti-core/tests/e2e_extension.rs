//! The extension's real backup code (run with Node) and this crate, through a real WebSocket relay
//! on localhost. Needs ../gooti-extension with node_modules, so it only runs on request:
//! `cargo test -p gooti-core --test e2e_extension -- --ignored`

mod common;

use std::path::PathBuf;
use std::process::Command;

use common::{Mode, start_relay};
use gooti_core::backup::flow::{back_up_shared, fetch_shared_document};
use gooti_core::backup::merge::merge_shared_documents;
use gooti_core::backup::phrase::{derive_shared_keys, generate_passphrase};
use gooti_core::backup::relay::WebSocketRelays;
use gooti_core::backup::state::{LiveIdentity, project_shared_document};
use serde_json::Value;

fn extension(mode: &str, relay: &str, phrase: &str) -> Value {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(root.join("tools/vectors/run.sh"))
        .args(["e2e", mode, relay, phrase])
        .output()
        .expect("node and the extension checkout are needed");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn the_extension_and_the_desktop_sync_through_one_relay() {
    let (relay, stored) = start_relay(Mode::Normal).await;
    let phrase = generate_passphrase().to_string();
    let relays = vec![relay.clone()];
    let io = WebSocketRelays::default();
    let keys = derive_shared_keys(&phrase).unwrap();

    // 1. The browser sends its vault.
    let started = tokio::task::spawn_blocking({
        let (relay, phrase) = (relay.clone(), phrase.clone());
        move || extension("start", &relay, &phrase)
    })
    .await
    .unwrap();
    assert_eq!(started["published"], true);
    assert_eq!(stored.lock().unwrap().len(), 1);

    // 2. The desktop reads it, adds its own identity, and backups.
    let remote = fetch_shared_document(&keys, &relays, &io)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(remote.identities[0].nick, "From the extension");
    let desktop = LiveIdentity {
        pubkey: "4f355bdcb7cc0af728ef3cceb9615d90684bb5b2ca5f859ab0f0b704075871aa".into(),
        privkey: Some("11".repeat(32)),
        nick: "From the desktop".into(),
        created_at: "2026-11-11T09:00:00.000Z".into(),
        color: None,
        bunker: None,
        custody_at: None,
    };
    let (local, _) = project_shared_document(
        &merge_shared_documents(&Default::default(), &remote),
        &[desktop],
        1e13,
    );
    let result = back_up_shared(&local, &keys, &relays, &io).await.unwrap();
    assert!(result.published);
    assert_eq!(result.report.sent, ["Added identity From the desktop"]);

    // 3. A fresh browser joins and gets both.
    let checked = tokio::task::spawn_blocking({
        let (relay, phrase) = (relay.clone(), phrase.clone());
        move || extension("check", &relay, &phrase)
    })
    .await
    .unwrap();
    let mut nicks: Vec<String> = serde_json::from_value(checked["nicks"].clone()).unwrap();
    nicks.sort();
    assert_eq!(nicks, ["From the desktop", "From the extension"]);
}

/// The extension's own bunker client (nostr-tools BunkerSigner) against the desktop bunker.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn the_extension_signs_through_the_desktop_bunker() {
    use std::sync::{Arc, Mutex};

    use gooti_core::bunker::service::{BunkerService, Notice};
    use gooti_core::{Gooti, KdfParams, MemoryKeychain, NewKey};

    let (relay, _) = start_relay(Mode::Normal).await;
    let dir = tempfile::tempdir().unwrap();
    let mut gooti = Gooti::open_with_kdf(
        dir.path(),
        Box::new(MemoryKeychain::default()),
        KdfParams::TEST,
    );
    gooti.vault_mut().create("1234").unwrap();
    let alice = gooti
        .vault_mut()
        .add_identity("Alice", NewKey::Generate)
        .unwrap();
    gooti
        .vault_mut()
        .bunker_set_relays(std::slice::from_ref(&relay))
        .unwrap();
    let link = gooti.vault_mut().bunker_link(&alice.id).unwrap();
    let gooti = Arc::new(Mutex::new(gooti));

    // Approves every request, as a user pressing Approve would.
    let (tx, mut asks) = tokio::sync::mpsc::unbounded_channel();
    let service = BunkerService::start(
        Arc::clone(&gooti),
        Arc::new(move |notice| {
            if let Notice::Asked { ask } = notice {
                let _ = tx.send(ask.key);
            }
        }),
    );
    let approver = Arc::clone(&service);
    let approving = tokio::spawn(async move {
        let mut approved = 0;
        while let Some(key) = asks.recv().await {
            approver
                .decide(&key, true, gooti_core::bunker::policy::Remember::Once)
                .unwrap();
            approved += 1;
        }
        approved
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let result = tokio::task::spawn_blocking(move || {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let output = Command::new(root.join("tools/vectors/run.sh"))
            .args(["bunker", &link])
            .output()
            .expect("node and the extension checkout are needed");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    })
    .await
    .unwrap();

    assert_eq!(result["pubkey"], alice.pubkey);
    // The link says `gooti=1`, so the extension knows Gooti Desktop holds the key.
    assert_eq!(result["gootiDesktop"], true);
    assert_eq!(result["signedBy"], alice.pubkey);
    assert_eq!(result["valid"], true);
    assert_eq!(result["content"], "signed by the desktop");
    assert_eq!(result["plaintext"], "round trip");
    assert_eq!(result["relays"], serde_json::json!({ "changed": false }));
    let info = gooti.lock().unwrap().vault().bunker_info().unwrap();
    assert_eq!(info.connections.len(), 1);

    service.stop();
    drop(service);
    approving.abort();
}

/// Desktop only: a fresh browser applies the desktop's vault with the extension's own code,
/// gets a bunker identity without the key, and signs through the desktop with the shared
/// session key, without a link and without a prompt (the browsers' connection is Basic).
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn a_browser_signs_through_the_desktop_with_the_shared_pointer() {
    use std::sync::{Arc, Mutex};

    use gooti_core::backup::blob::seal_shared_document;
    use gooti_core::bunker::service::BunkerService;
    use gooti_core::{Custody, Gooti, KdfParams, MemoryKeychain, NewKey};

    let (relay, _) = start_relay(Mode::Normal).await;
    let dir = tempfile::tempdir().unwrap();
    let mut gooti = Gooti::open_with_kdf(
        dir.path(),
        Box::new(MemoryKeychain::default()),
        KdfParams::TEST,
    );
    let vault = gooti.vault_mut();
    vault.create("1234").unwrap();
    // The pointer gets the wss:// relay; the bunker also listens on the local test relay.
    vault
        .bunker_set_relays(&["wss://relay.invalid".into(), relay.clone()])
        .unwrap();
    let phrase = generate_passphrase().to_string();
    vault
        .enable_backup(&phrase, &["wss://relay.invalid".into()])
        .unwrap();
    let privkey = "31".repeat(32);
    let alice = vault
        .add_identity_as(
            "Alice",
            NewKey::Import {
                key: &privkey,
                password: None,
            },
            Custody::Desktop,
        )
        .unwrap();
    let document = vault.shared_plan().unwrap().document;
    let keys = derive_shared_keys(&phrase).unwrap();
    let payload = seal_shared_document(&document, &keys.encryption_key).unwrap();

    let gooti = Arc::new(Mutex::new(gooti));
    let service = BunkerService::start(Arc::clone(&gooti), Arc::new(|_| {}));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let input = dir.path().join("custody.json");
    std::fs::write(
        &input,
        serde_json::json!({
            "payload": payload,
            "encryptionKeyHex": keys.encryption_hex().as_str(),
            "relay": relay,
            "privkey": privkey,
        })
        .to_string(),
    )
    .unwrap();
    let result = tokio::task::spawn_blocking(move || {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let output = Command::new(root.join("tools/vectors/run.sh"))
            .args(["custody", input.to_str().unwrap()])
            .output()
            .expect("node and the extension checkout are needed");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    })
    .await
    .unwrap();

    assert_eq!(result["nick"], "Alice");
    assert_eq!(result["hasPrivkey"], false);
    assert_eq!(result["signedBy"], alice.pubkey);
    assert_eq!(result["valid"], true);
    assert!(
        service.pending().is_empty(),
        "Basic signs a note without asking"
    );
    // 2.1: the pointer carries the custody time, and a browser's newer copy of the key loses.
    assert!(result["custodyAt"].is_number());
    assert_eq!(result["staleKeyLoses"], true);
    // The extension named the site; the desktop logged it.
    let activity = gooti.lock().unwrap().vault().activity().unwrap();
    let logged = serde_json::to_value(&activity).unwrap();
    assert!(
        logged
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["type"] == "request" && entry["site"] == "primal.net"),
        "{logged}"
    );
    service.stop();
}

/// After "Reset Gooti" and "Restore from backup", the desktop answers the pointer the browser
/// still holds from before: the desktop record brought back the key and the same signer key.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn a_browser_signs_through_a_desktop_restored_after_a_reset() {
    use std::sync::{Arc, Mutex};

    use common::MemoryRelays;
    use gooti_core::backup::flow::BackupRunner;
    use gooti_core::bunker::service::BunkerService;
    use gooti_core::{Custody, Gooti, KdfParams, MemoryKeychain, NewKey};

    const BACKUP: &str = "wss://relay.invalid";
    let (relay, _) = start_relay(Mode::Normal).await;
    let backup = MemoryRelays::default();
    let phrase = generate_passphrase().to_string();
    let privkey = "31".repeat(32);

    // Before the reset: Alice is Desktop only, and the backup has both records.
    let before_dir = tempfile::tempdir().unwrap();
    let before = Arc::new(Mutex::new(Gooti::open_with_kdf(
        before_dir.path(),
        Box::new(MemoryKeychain::default()),
        KdfParams::TEST,
    )));
    let alice = {
        let mut gooti = before.lock().unwrap();
        let vault = gooti.vault_mut();
        vault.create("1234").unwrap();
        vault
            .bunker_set_relays(&[BACKUP.into(), relay.clone()])
            .unwrap();
        vault.enable_backup(&phrase, &[BACKUP.into()]).unwrap();
        vault
            .add_identity_as(
                "Alice",
                NewKey::Import {
                    key: &privkey,
                    password: None,
                },
                Custody::Desktop,
            )
            .unwrap()
    };
    BackupRunner::new(Arc::clone(&before), backup.clone())
        .back_up_now()
        .await
        .unwrap();
    // What the browser holds: the shared record, with the pointer to this desktop.
    let keys = derive_shared_keys(&phrase).unwrap();
    let payload = backup
        .events
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|(_, event)| event.pubkey == keys.signing.public_key())
        .map(|(_, event)| event.content.clone())
        .unwrap();
    drop(before);
    drop(before_dir);

    // The same computer after the reset.
    let after_dir = tempfile::tempdir().unwrap();
    let after = Arc::new(Mutex::new(Gooti::open_with_kdf(
        after_dir.path(),
        Box::new(MemoryKeychain::default()),
        KdfParams::TEST,
    )));
    BackupRunner::new(Arc::clone(&after), backup.clone())
        .restore_new_vault("1234", &phrase, &[BACKUP.into()])
        .await
        .unwrap();
    {
        let mut gooti = after.lock().unwrap();
        let ids: Vec<String> = gooti
            .vault()
            .identities()
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        gooti.vault_mut().serve_here(&ids).unwrap();
    }
    let service = BunkerService::start(Arc::clone(&after), Arc::new(|_| {}));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let input = after_dir.path().join("custody.json");
    std::fs::write(
        &input,
        serde_json::json!({
            "payload": payload,
            "encryptionKeyHex": keys.encryption_hex().as_str(),
            "relay": relay,
            "privkey": privkey,
        })
        .to_string(),
    )
    .unwrap();
    let result = tokio::task::spawn_blocking(move || {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let output = Command::new(root.join("tools/vectors/run.sh"))
            .args(["custody", input.to_str().unwrap()])
            .output()
            .expect("node and the extension checkout are needed");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    })
    .await
    .unwrap();

    assert_eq!(result["nick"], "Alice");
    assert_eq!(result["hasPrivkey"], false);
    assert_eq!(result["signedBy"], alice.pubkey);
    assert_eq!(result["valid"], true);
    service.stop();
}
