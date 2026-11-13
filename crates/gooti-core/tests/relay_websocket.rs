//! The WebSocket relay client against a real relay on localhost.

mod common;

use std::time::Duration;

use common::{Mode, start_relay};
use gooti_core::Error;
use gooti_core::backup::event::{SHARED_D_TAG, build_backup_event};
use gooti_core::backup::phrase::{derive_shared_keys, generate_passphrase};
use gooti_core::backup::relay::{RelayIo, WebSocketRelays};

fn relays(wait_ms: u64) -> WebSocketRelays {
    WebSocketRelays {
        wait: Some(Duration::from_millis(wait_ms)),
    }
}

#[tokio::test]
async fn publishes_then_fetches_the_newest_vault_event() {
    let (url, stored) = start_relay(Mode::Normal).await;
    let keys = derive_shared_keys(&generate_passphrase()).unwrap();
    let io = relays(2000);
    let relays = vec![url.clone()];

    assert_eq!(
        io.fetch_newest(&relays, &keys.pubkey_hex(), SHARED_D_TAG)
            .await
            .unwrap(),
        None
    );

    let older = build_backup_event(&keys, SHARED_D_TAG, "older", 1_000);
    let newer = build_backup_event(&keys, SHARED_D_TAG, "newer", 2_000);
    assert_eq!(io.publish(&relays, &newer).await.unwrap(), relays);
    io.publish(&relays, &older).await.unwrap();
    assert_eq!(stored.lock().unwrap().len(), 2);

    let fetched = io
        .fetch_newest(&relays, &keys.pubkey_hex(), SHARED_D_TAG)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.content, "newer");
}

#[tokio::test]
async fn one_working_relay_is_enough_and_all_failing_lists_each_reason() {
    let (good, _) = start_relay(Mode::Normal).await;
    let (rejecting, _) = start_relay(Mode::Reject).await;
    let unreachable = "ws://127.0.0.1:9".to_string();
    let keys = derive_shared_keys(&generate_passphrase()).unwrap();
    let event = build_backup_event(&keys, SHARED_D_TAG, "vault", 1_000);
    let io = relays(2000);

    let accepted = io
        .publish(
            &[unreachable.clone(), rejecting.clone(), good.clone()],
            &event,
        )
        .await
        .unwrap();
    assert_eq!(accepted, [good]);

    let error = io
        .publish(&[unreachable.clone(), rejecting.clone()], &event)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "No relay accepted your backup.");
    let failures = error.relay_failures();
    assert_eq!(failures.len(), 2);
    assert_eq!(failures[0].url, unreachable);
    assert!(!failures[0].reason.is_empty());
    assert_eq!(failures[1].reason, "blocked: only vault events");

    let error = io
        .fetch_newest(&[unreachable], &keys.pubkey_hex(), SHARED_D_TAG)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Relays { .. }));
    assert_eq!(error.to_string(), "No relay could be reached.");
}

#[tokio::test]
async fn a_relay_without_eose_ends_the_wait_with_what_it_sent() {
    let (url, stored) = start_relay(Mode::Silent).await;
    let keys = derive_shared_keys(&generate_passphrase()).unwrap();
    stored
        .lock()
        .unwrap()
        .push(build_backup_event(&keys, SHARED_D_TAG, "vault", 1_000));
    let fetched = relays(300)
        .fetch_newest(&[url], &keys.pubkey_hex(), SHARED_D_TAG)
        .await
        .unwrap();
    assert_eq!(fetched.unwrap().content, "vault");
}

/// Reaches the real default relay over TLS. Read only: asks for a random key's vault.
/// Run on request: `cargo test -p gooti-core --test relay_websocket -- --ignored`
#[tokio::test]
#[ignore]
async fn reaches_the_default_relay_over_tls() {
    let keys = derive_shared_keys(&generate_passphrase()).unwrap();
    let relays = gooti_core::backup::flow::default_relays();
    let found = WebSocketRelays::default()
        .fetch_newest(&relays, &keys.pubkey_hex(), SHARED_D_TAG)
        .await;
    assert_eq!(found.unwrap(), None);
}
