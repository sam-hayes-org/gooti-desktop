//! Whole backup flows between desktops, and with a vault the extension sealed.

mod common;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use common::MemoryRelays;
use gooti_core::backup::flow::{BackupResult, BackupRunner};
use gooti_core::backup::relay::RelayIo;
use gooti_core::{Custody, Error, Gooti, IdentitySummary, KdfParams, MemoryKeychain, NewKey};
use nostr::event::Event;
use serde_json::Value;

const RELAY: &str = "wss://relay.example";
const HEX: &str = "67dea2ed018072d675f5415ecfaed7d2597555e202d85b3d65ea4e58d2d92ffa";

struct Desktop {
    gooti: Arc<Mutex<Gooti>>,
    backup_runner: BackupRunner<MemoryRelays>,
    _dir: tempfile::TempDir,
}

impl Desktop {
    fn new(relays: &MemoryRelays) -> Desktop {
        let dir = tempfile::tempdir().unwrap();
        let gooti = Arc::new(Mutex::new(Gooti::open_with_kdf(
            dir.path(),
            Box::new(MemoryKeychain::default()),
            KdfParams::TEST,
        )));
        let backup_runner = BackupRunner::new(Arc::clone(&gooti), relays.clone());
        Desktop {
            gooti,
            backup_runner,
            _dir: dir,
        }
    }

    fn with_vault(relays: &MemoryRelays) -> Desktop {
        let desktop = Desktop::new(relays);
        desktop
            .gooti
            .lock()
            .unwrap()
            .vault_mut()
            .create("1234")
            .unwrap();
        desktop
    }

    fn nicks(&self) -> Vec<String> {
        let gooti = self.gooti.lock().unwrap();
        gooti
            .vault()
            .identities()
            .unwrap()
            .into_iter()
            .map(|i| i.nick)
            .collect()
    }

    fn add(&self, nick: &str) -> String {
        let mut gooti = self.gooti.lock().unwrap();
        gooti
            .vault_mut()
            .add_identity(nick, NewKey::Generate)
            .unwrap()
            .id
    }
}

/// Events of the shared record, the one browsers read.
fn shared_events(relays: &MemoryRelays, phrase: &str) -> usize {
    let shared = gooti_core::backup::phrase::derive_shared_keys(phrase)
        .unwrap()
        .pubkey_hex();
    relays
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, e)| e.pubkey.to_hex() == shared)
        .count()
}

fn phrase() -> String {
    gooti_core::backup::phrase::generate_passphrase().to_string()
}

#[tokio::test]
async fn two_desktops_share_identities_renames_and_deletes() {
    let relays = MemoryRelays::default();
    let phrase = phrase();
    let a = Desktop::with_vault(&relays);
    let alice = a.add("Alice");
    a.gooti
        .lock()
        .unwrap()
        .vault_mut()
        .enable_backup(&phrase, &[RELAY.into()])
        .unwrap();

    let first = a.backup_runner.back_up_now().await.unwrap();
    assert_eq!(first.result, BackupResult::BackedUp);
    assert_eq!(first.report.sent, ["Created the backup."]);
    assert_eq!(
        a.backup_runner.back_up_now().await.unwrap().result,
        BackupResult::Current
    );

    let b = Desktop::with_vault(&relays);
    b.add("Bob");
    b.backup_runner
        .join(&phrase, &[RELAY.into()])
        .await
        .unwrap();
    assert_eq!(b.nicks(), ["Bob", "Alice"]);

    let outcome = a.backup_runner.back_up_now().await.unwrap();
    assert_eq!(outcome.result, BackupResult::Updated);
    assert_eq!(outcome.report.took, ["Added identity Bob"]);

    std::thread::sleep(std::time::Duration::from_millis(3));
    a.gooti
        .lock()
        .unwrap()
        .vault_mut()
        .rename_identity(&alice, "Alicia")
        .unwrap();
    assert_eq!(
        a.backup_runner.back_up_now().await.unwrap().report.sent,
        ["Renamed Alice to Alicia"]
    );
    assert_eq!(
        b.backup_runner.back_up_now().await.unwrap().report.took,
        ["Renamed Alice to Alicia"]
    );
    assert_eq!(b.nicks(), ["Bob", "Alicia"]);

    a.gooti
        .lock()
        .unwrap()
        .vault_mut()
        .delete_identity(&alice)
        .unwrap();
    a.backup_runner.back_up_now().await.unwrap();
    let outcome = b.backup_runner.back_up_now().await.unwrap();
    assert_eq!(outcome.report.took, ["Removed identity Alicia"]);
    assert_eq!(b.nicks(), ["Bob"]);
}

#[tokio::test]
async fn joining_without_a_relay_vault_changes_nothing() {
    let relays = MemoryRelays::default();
    let desktop = Desktop::with_vault(&relays);
    desktop.add("Alice");
    let error = desktop
        .backup_runner
        .join(&phrase(), &[RELAY.into()])
        .await
        .unwrap_err();
    assert!(matches!(error, Error::NoRemoteVault));
    assert!(
        desktop
            .gooti
            .lock()
            .unwrap()
            .vault()
            .backup_info()
            .unwrap()
            .is_none()
    );
    assert_eq!(relays.count(), 0);
}

#[tokio::test]
async fn the_wrong_passphrase_cannot_open_the_relay_vault() {
    let relays = MemoryRelays::default();
    let a = Desktop::with_vault(&relays);
    let phrase = phrase();
    a.gooti
        .lock()
        .unwrap()
        .vault_mut()
        .enable_backup(&phrase, &[RELAY.into()])
        .unwrap();
    a.backup_runner.back_up_now().await.unwrap();
    // Copy A's event under another key's name is not possible; a wrong phrase simply finds nothing.
    let b = Desktop::with_vault(&relays);
    assert!(matches!(
        b.backup_runner.join(&self::phrase(), &[RELAY.into()]).await,
        Err(Error::NoRemoteVault)
    ));
}

#[tokio::test]
async fn relay_failures_leave_the_vault_as_it_was() {
    let relays = MemoryRelays::default();
    let desktop = Desktop::with_vault(&relays);
    desktop.add("Alice");
    desktop
        .gooti
        .lock()
        .unwrap()
        .vault_mut()
        .enable_backup(&phrase(), &[RELAY.into()])
        .unwrap();
    relays.down.lock().unwrap().push(RELAY.into());
    let error = desktop.backup_runner.back_up_now().await.unwrap_err();
    assert_eq!(error.to_string(), "No relay could be reached.");
    assert_eq!(desktop.nicks(), ["Alice"]);
}

#[tokio::test]
async fn restoring_a_new_vault_needs_a_relay_vault_first() {
    let relays = MemoryRelays::default();
    let phrase = phrase();
    let fresh = Desktop::new(&relays);
    let error = fresh
        .backup_runner
        .restore_new_vault("1234", &phrase, &[RELAY.into()])
        .await
        .unwrap_err();
    assert!(matches!(error, Error::NoRemoteVault));
    assert_eq!(
        fresh.gooti.lock().unwrap().vault().status(),
        gooti_core::VaultStatus::NoVault
    );

    let a = Desktop::with_vault(&relays);
    a.add("Alice");
    a.gooti
        .lock()
        .unwrap()
        .vault_mut()
        .enable_backup(&phrase, &[RELAY.into()])
        .unwrap();
    a.backup_runner.back_up_now().await.unwrap();

    fresh
        .backup_runner
        .restore_new_vault("1234", &phrase, &[RELAY.into()])
        .await
        .unwrap();
    assert_eq!(fresh.nicks(), ["Alice"]);
    let mut gooti = fresh.gooti.lock().unwrap();
    gooti.vault_mut().lock();
    gooti.vault_mut().unlock("1234").unwrap();
    assert!(gooti.vault().backup_info().unwrap().is_some());
}

#[tokio::test]
async fn a_desktop_joins_the_vault_the_extension_sealed() {
    let vectors: Value = serde_json::from_str(
        &std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/extension.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let event: Event = serde_json::from_value(vectors["blob"]["event"].clone()).unwrap();
    let phrase = vectors["blob"]["phrase"].as_str().unwrap();
    let relays = MemoryRelays::default();
    relays.put(RELAY, event);

    let desktop = Desktop::with_vault(&relays);
    desktop
        .gooti
        .lock()
        .unwrap()
        .vault_mut()
        .add_identity(
            "Desktop",
            NewKey::Import {
                key: HEX,
                password: None,
            },
        )
        .unwrap();
    desktop
        .backup_runner
        .join(phrase, &[RELAY.into()])
        .await
        .unwrap();

    // Alice and Bob carry keys, Phone is a bunker; Carol was deleted in the extension.
    let gooti = desktop.gooti.lock().unwrap();
    let identities = gooti.vault().identities().unwrap();
    let nicks: Vec<_> = identities.iter().map(|i| i.nick.as_str()).collect();
    assert_eq!(nicks, ["Desktop", "Alice", "Bob", "Phone"]);
    let phone = identities.iter().find(|i| i.nick == "Phone").unwrap();
    assert!(phone.is_bunker && !phone.has_private_key);
    assert_eq!(
        identities
            .iter()
            .find(|i| i.nick == "Alice")
            .unwrap()
            .color
            .as_deref(),
        Some("#a1b2c3")
    );
    drop(gooti);
    // The desktop's own identity went back to the relay; the extension's rows are untouched.
    assert_eq!(shared_events(&relays, phrase), 2);
    // The desktop record went out too, under its own key.
    assert_eq!(relays.count(), 3);
}

/// A vault event is replaceable: of two in the same second, relays keep only one. Each event the
/// desktop sends must be later than the one it merged with, even when a backup follows at once.
#[tokio::test]
async fn each_published_vault_is_later_than_the_one_it_merged() {
    let relays = MemoryRelays::default();
    let phrase = phrase();
    let a = Desktop::with_vault(&relays);
    a.gooti
        .lock()
        .unwrap()
        .vault_mut()
        .enable_backup(&phrase, &[RELAY.into()])
        .unwrap();
    let b = Desktop::with_vault(&relays);
    a.add("A1");
    a.backup_runner.back_up_now().await.unwrap();
    b.backup_runner
        .join(&phrase, &[RELAY.into()])
        .await
        .unwrap();
    b.add("B1");
    b.backup_runner.back_up_now().await.unwrap();
    a.add("A2");
    a.backup_runner.back_up_now().await.unwrap();

    // Per record: the shared record and the desktop record each replace only their own.
    let shared = gooti_core::backup::phrase::derive_shared_keys(&phrase)
        .unwrap()
        .pubkey_hex();
    let times: Vec<u64> = relays
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, e)| e.pubkey.to_hex() == shared)
        .map(|(_, e)| e.created_at.as_secs())
        .collect();
    assert!(times.len() >= 3, "{times:?}");
    assert!(times.windows(2).all(|pair| pair[1] > pair[0]), "{times:?}");
    b.backup_runner.back_up_now().await.unwrap();
    assert_eq!(b.nicks(), ["A1", "B1", "A2"]);
}

/// A browser still backups Alice with her key; this desktop holds Alice as Desktop only and joins.
/// Joining replaces the key with the pointer and sends it at once (and must not hang doing so).
#[tokio::test]
async fn joining_with_a_desktop_only_identity_sends_the_pointer_over_the_shared_key() {
    let relays = MemoryRelays::default();
    let phrase = phrase();

    // The browser side: Alice with her key in the relay vault.
    let browser = Desktop::with_vault(&relays);
    browser
        .gooti
        .lock()
        .unwrap()
        .vault_mut()
        .add_identity(
            "Alice",
            NewKey::Import {
                key: HEX,
                password: None,
            },
        )
        .unwrap();
    browser
        .gooti
        .lock()
        .unwrap()
        .vault_mut()
        .enable_backup(&phrase, &[RELAY.into()])
        .unwrap();
    browser.backup_runner.back_up_now().await.unwrap();

    // This desktop: Alice kept here only.
    let desktop = Desktop::with_vault(&relays);
    desktop
        .gooti
        .lock()
        .unwrap()
        .vault_mut()
        .add_identity_as(
            "Alice",
            NewKey::Import {
                key: HEX,
                password: None,
            },
            gooti_core::Custody::Desktop,
        )
        .unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        desktop.backup_runner.join(&phrase, &[RELAY.into()]),
    )
    .await
    .expect("joining hung")
    .unwrap();

    // The relay vault now carries the pointer for Alice, not her key. (A browser drops its key
    // when it takes that row; another Gooti desktop keeps a key it holds, by design.)
    let keys = gooti_core::backup::phrase::derive_shared_keys(&phrase).unwrap();
    let remote = gooti_core::backup::flow::fetch_shared_document(&keys, &[RELAY.into()], &relays)
        .await
        .unwrap()
        .unwrap();
    let row = &remote.identities[0];
    assert!(
        row.privkey.is_none(),
        "the relay vault no longer carries the key"
    );
    assert!(
        row.bunker
            .as_ref()
            .is_some_and(|b| b.client_secret_key.is_some())
    );
    assert!(!serde_json::to_string(&remote).unwrap().contains(HEX));

    let gooti = desktop.gooti.lock().unwrap();
    let alice = &gooti.vault().identities().unwrap()[0];
    assert!(alice.has_private_key && alice.custody == gooti_core::Custody::Desktop);
}

#[tokio::test]
async fn busy_while_a_sync_runs_and_also_when_it_fails() {
    let relays = MemoryRelays::default();
    let a = Desktop::with_vault(&relays);
    a.gooti
        .lock()
        .unwrap()
        .vault_mut()
        .enable_backup(&phrase(), &[RELAY.into()])
        .unwrap();
    let mut updates = a.backup_runner.busy_updates();
    assert!(!a.backup_runner.is_busy());

    // Two overlapping backups: busy once at the start, idle once after both.
    let (first, second) =
        tokio::join!(a.backup_runner.back_up_now(), a.backup_runner.back_up_now());
    first.unwrap();
    second.unwrap();
    assert!(!a.backup_runner.is_busy());
    assert!(updates.has_changed().unwrap());
    assert!(!*updates.borrow_and_update());

    // A restore that fails ends busy too.
    let b = Desktop::with_vault(&relays);
    let seen = b.backup_runner.busy_updates();
    assert!(
        b.backup_runner
            .join(&phrase(), &[RELAY.into()])
            .await
            .is_err()
    );
    assert!(seen.has_changed().unwrap());
    assert!(!b.backup_runner.is_busy());
}

/// A relay set that refuses the desktop record and takes everything else.
#[derive(Clone)]
struct RefuseDesktopRecord {
    inner: MemoryRelays,
    desktop_pubkey: String,
}

impl RelayIo for RefuseDesktopRecord {
    async fn publish(&self, relays: &[String], event: &Event) -> gooti_core::Result<Vec<String>> {
        if event.pubkey.to_hex() == self.desktop_pubkey {
            return Err(Error::Relays {
                message: "No relay accepted your backup.".into(),
                failures: Vec::new(),
            });
        }
        self.inner.publish(relays, event).await
    }

    async fn fetch_newest(
        &self,
        relays: &[String],
        pubkey: &str,
        d_tag: &str,
    ) -> gooti_core::Result<Option<Event>> {
        self.inner.fetch_newest(relays, pubkey, d_tag).await
    }
}

fn find<'a>(identities: &'a [IdentitySummary], nick: &str) -> &'a IdentitySummary {
    identities.iter().find(|i| i.nick == nick).unwrap()
}

fn identities(desktop: &Desktop) -> Vec<IdentitySummary> {
    desktop.gooti.lock().unwrap().vault().identities().unwrap()
}

fn signer_of(desktop: &Desktop, identity: &IdentitySummary) -> String {
    let gooti = desktop.gooti.lock().unwrap();
    gooti
        .vault()
        .bunker_signer(&identity.pubkey)
        .unwrap()
        .unwrap()
        .public_key()
        .to_hex()
}

fn listens(desktop: &Desktop) -> Vec<String> {
    desktop
        .gooti
        .lock()
        .unwrap()
        .vault()
        .bunker_listen()
        .signer_pubkeys
}

/// For each app: whether another Gooti Desktop answers it.
fn apps_served_elsewhere(desktop: &Desktop) -> Vec<bool> {
    let gooti = desktop.gooti.lock().unwrap();
    let info = gooti.vault().bunker_info().unwrap();
    info.connections
        .iter()
        .map(|c| c.served_elsewhere)
        .collect()
}

fn app_names(desktop: &Desktop) -> Vec<String> {
    let gooti = desktop.gooti.lock().unwrap();
    let mut names: Vec<String> = gooti
        .vault()
        .bunker_info()
        .unwrap()
        .connections
        .into_iter()
        .filter_map(|c| c.name)
        .collect();
    names.sort();
    names
}

fn connect_app(desktop: &Desktop, identity_id: &str, name: &str) {
    let client = nostr::key::Keys::generate().public_key().to_hex();
    let link =
        format!("nostrconnect://{client}?relay=wss%3A%2F%2Fapp.relay&secret=abc&name={name}");
    let mut gooti = desktop.gooti.lock().unwrap();
    gooti
        .vault_mut()
        .bunker_accept_nostrconnect(&link, identity_id)
        .unwrap();
}

/// The shared record on the relays, as a browser reads it.
async fn shared_row(relays: &MemoryRelays, phrase: &str, nick: &str) -> Value {
    let keys = gooti_core::backup::phrase::derive_shared_keys(phrase).unwrap();
    let document = gooti_core::backup::flow::fetch_shared_document(&keys, &[RELAY.into()], relays)
        .await
        .unwrap()
        .unwrap();
    let value = serde_json::to_value(&document).unwrap();
    value["identities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["nick"] == nick)
        .unwrap()
        .clone()
}

/// A desktop with Alice (Desktop only), Bob (shared, with an app), and backup on, backed up once.
async fn backed_up(relays: &MemoryRelays, phrase: &str) -> Desktop {
    let desktop = Desktop::with_vault(relays);
    let bob = {
        let mut gooti = desktop.gooti.lock().unwrap();
        let vault = gooti.vault_mut();
        vault
            .add_identity_as("Alice", NewKey::Generate, Custody::Desktop)
            .unwrap();
        let bob = vault.add_identity("Bob", NewKey::Generate).unwrap().id;
        vault.enable_backup(phrase, &[RELAY.into()]).unwrap();
        bob
    };
    connect_app(&desktop, &bob, "Coracle");
    desktop.backup_runner.back_up_now().await.unwrap();
    desktop
}

#[tokio::test]
async fn a_restore_after_a_reset_brings_back_desktop_only_keys_signers_and_apps() {
    let relays = MemoryRelays::default();
    let phrase = phrase();
    let before = backed_up(&relays, &phrase).await;
    let old = identities(&before);
    let alice_signer = signer_of(&before, find(&old, "Alice"));
    let bob_signer = signer_of(&before, find(&old, "Bob"));
    let pointer = shared_row(&relays, &phrase, "Alice").await;
    assert!(
        pointer.get("privkey").is_none(),
        "the browsers never get Alice's key"
    );

    // The same desktop after "Reset Gooti" and "Restore from backup".
    let after = Desktop::new(&relays);
    after
        .backup_runner
        .restore_new_vault("1234", &phrase, &[RELAY.into()])
        .await
        .unwrap();
    let restored = identities(&after);
    let alice = find(&restored, "Alice");
    assert!(alice.has_private_key && !alice.is_bunker && !alice.key_missing);
    assert_eq!(alice.custody, Custody::Desktop);
    assert_eq!(alice.pubkey, find(&old, "Alice").pubkey);
    // The same signer keys, so the browsers' pointer and Coracle's link still reach it.
    assert_eq!(signer_of(&after, alice), alice_signer);
    assert_eq!(signer_of(&after, find(&restored, "Bob")), bob_signer);
    assert_eq!(app_names(&after), ["Coracle", "Gooti browsers"]);
    // A new desktop: it waits to be told to sign, in case the old one still runs.
    assert!(alice.served_elsewhere && find(&restored, "Bob").served_elsewhere);
    assert!(listens(&after).is_empty());
    // Connections shows that for every app, the browsers included.
    assert_eq!(apps_served_elsewhere(&after), [true, true]);
    {
        let mut gooti = after.gooti.lock().unwrap();
        let ids: Vec<String> = restored.iter().map(|i| i.id.clone()).collect();
        gooti.vault_mut().serve_here(&ids).unwrap();
    }
    let mut listening = listens(&after);
    listening.sort();
    let mut expected = vec![alice_signer, bob_signer];
    expected.sort();
    assert_eq!(listening, expected);
    assert!(!find(&identities(&after), "Alice").served_elsewhere);
    assert_eq!(apps_served_elsewhere(&after), [false, false]);
    assert_eq!(apps_served_elsewhere(&before), [false, false]);

    // Backing up again keeps the pointer the browsers hold.
    after.backup_runner.back_up_now().await.unwrap();
    let again = shared_row(&relays, &phrase, "Alice").await;
    assert_eq!(again["bunker"]["pubkey"], pointer["bunker"]["pubkey"]);
    assert!(again.get("privkey").is_none());
}

#[tokio::test]
async fn without_the_desktop_record_a_desktop_only_key_is_missing() {
    let relays = MemoryRelays::default();
    let phrase = phrase();
    let _before = backed_up(&relays, &phrase).await;
    let desktop_pubkey = gooti_core::backup::phrase::derive_desktop_keys(&phrase)
        .unwrap()
        .pubkey_hex();
    relays
        .events
        .lock()
        .unwrap()
        .retain(|(_, event)| event.pubkey.to_hex() != desktop_pubkey);

    let after = Desktop::new(&relays);
    after
        .backup_runner
        .restore_new_vault("1234", &phrase, &[RELAY.into()])
        .await
        .unwrap();
    let restored = identities(&after);
    let alice = find(&restored, "Alice");
    assert!(!alice.has_private_key && alice.is_bunker && alice.key_missing);
    let bob = find(&restored, "Bob");
    assert!(bob.has_private_key && !bob.key_missing);
}

#[tokio::test]
async fn a_moved_key_stays_in_the_shared_record_until_the_desktop_record_is_sent() {
    let relays = MemoryRelays::default();
    let phrase = phrase();
    let desktop = Desktop::with_vault(&relays);
    let bob = {
        let mut gooti = desktop.gooti.lock().unwrap();
        let vault = gooti.vault_mut();
        let bob = vault.add_identity("Bob", NewKey::Generate).unwrap().id;
        vault.enable_backup(&phrase, &[RELAY.into()]).unwrap();
        bob
    };
    let refusing = RefuseDesktopRecord {
        inner: relays.clone(),
        desktop_pubkey: gooti_core::backup::phrase::derive_desktop_keys(&phrase)
            .unwrap()
            .pubkey_hex(),
    };
    let backup_runner = BackupRunner::new(Arc::clone(&desktop.gooti), refusing);
    // Nothing goes out while the desktop record cannot.
    assert!(backup_runner.back_up_now().await.is_err());
    assert_eq!(relays.count(), 0);

    desktop.backup_runner.back_up_now().await.unwrap();
    desktop
        .gooti
        .lock()
        .unwrap()
        .vault_mut()
        .set_custody(&bob, Custody::Desktop)
        .unwrap();
    assert!(backup_runner.back_up_now().await.is_err());
    // The browsers still have Bob's key from the shared record.
    let row = shared_row(&relays, &phrase, "Bob").await;
    assert!(row["privkey"].is_string());
    assert!(
        !desktop
            .gooti
            .lock()
            .unwrap()
            .vault()
            .backup_info()
            .unwrap()
            .unwrap()
            .desktop_current
    );

    desktop.backup_runner.back_up_now().await.unwrap();
    let row = shared_row(&relays, &phrase, "Bob").await;
    assert!(row.get("privkey").is_none() && row["bunker"].is_object());
    assert!(
        desktop
            .gooti
            .lock()
            .unwrap()
            .vault()
            .backup_info()
            .unwrap()
            .unwrap()
            .desktop_current
    );
}

#[tokio::test]
async fn two_desktops_share_the_desktop_record_and_one_answers() {
    let relays = MemoryRelays::default();
    let phrase = phrase();
    let a = backed_up(&relays, &phrase).await;
    let b = Desktop::with_vault(&relays);
    b.backup_runner
        .join(&phrase, &[RELAY.into()])
        .await
        .unwrap();

    let on_b = identities(&b);
    let alice = find(&on_b, "Alice");
    assert!(alice.has_private_key && alice.custody == Custody::Desktop);
    assert!(alice.served_elsewhere);
    assert_eq!(app_names(&b), ["Coracle", "Gooti browsers"]);
    assert!(listens(&b).is_empty());
    assert_eq!(listens(&a).len(), 2);

    // "Sign here instead" on B: A stops at its next backup.
    b.gooti
        .lock()
        .unwrap()
        .vault_mut()
        .serve_here(std::slice::from_ref(&alice.id))
        .unwrap();
    b.backup_runner.back_up_now().await.unwrap();
    a.backup_runner.back_up_now().await.unwrap();
    assert_eq!(listens(&b), [signer_of(&b, alice)]);
    assert_eq!(listens(&a), [signer_of(&a, find(&identities(&a), "Bob"))]);
    assert!(find(&identities(&a), "Alice").served_elsewhere);

    // An app removed on A is removed on B too.
    let coracle = {
        let gooti = a.gooti.lock().unwrap();
        let info = gooti.vault().bunker_info().unwrap();
        info.connections
            .into_iter()
            .find(|c| c.name.as_deref() == Some("Coracle"))
            .unwrap()
            .id
    };
    a.gooti
        .lock()
        .unwrap()
        .vault_mut()
        .bunker_revoke(&coracle)
        .unwrap();
    a.backup_runner.back_up_now().await.unwrap();
    b.backup_runner.back_up_now().await.unwrap();
    assert_eq!(app_names(&b), ["Gooti browsers"]);
    assert_eq!(app_names(&a), ["Gooti browsers"]);
}

#[tokio::test]
async fn a_key_moved_on_one_desktop_moves_on_the_other() {
    let relays = MemoryRelays::default();
    let phrase = phrase();
    let a = backed_up(&relays, &phrase).await;
    let b = Desktop::with_vault(&relays);
    b.backup_runner
        .join(&phrase, &[RELAY.into()])
        .await
        .unwrap();

    // B moves Bob to the desktop. A follows, and an edit on A does not send the key again.
    let bob = find(&identities(&b), "Bob").id.clone();
    b.gooti
        .lock()
        .unwrap()
        .vault_mut()
        .set_custody(&bob, Custody::Desktop)
        .unwrap();
    b.backup_runner.back_up_now().await.unwrap();
    a.backup_runner.back_up_now().await.unwrap();
    let bob_on_a = find(&identities(&a), "Bob").clone();
    assert_eq!(bob_on_a.custody, Custody::Desktop);
    assert!(bob_on_a.has_private_key && bob_on_a.served_elsewhere);
    a.gooti
        .lock()
        .unwrap()
        .vault_mut()
        .rename_identity(&bob_on_a.id, "Robert")
        .unwrap();
    a.backup_runner.back_up_now().await.unwrap();
    let row = shared_row(&relays, &phrase, "Robert").await;
    assert!(row.get("privkey").is_none() && row["bunker"].is_object());

    // "Use in browsers again" on A: B follows.
    a.gooti
        .lock()
        .unwrap()
        .vault_mut()
        .set_custody(&bob_on_a.id, Custody::Shared)
        .unwrap();
    a.backup_runner.back_up_now().await.unwrap();
    b.backup_runner.back_up_now().await.unwrap();
    assert_eq!(find(&identities(&b), "Robert").custody, Custody::Shared);
    b.backup_runner.back_up_now().await.unwrap();
    let row = shared_row(&relays, &phrase, "Robert").await;
    assert!(row["privkey"].is_string());
}
