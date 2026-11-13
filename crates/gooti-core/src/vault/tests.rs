use super::*;
use crate::backup::document::{SharedBunker, SharedIdentity};
use crate::backup::merge::merge_shared_documents;
use crate::identity::Custody;

const HEX: &str = "67dea2ed018072d675f5415ecfaed7d2597555e202d85b3d65ea4e58d2d92ffa";
const PHRASE: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

fn vault_in(dir: &tempfile::TempDir) -> Vault {
    Vault::with_kdf(dir.path().join("vault.json"), KdfParams::TEST)
}

fn unlocked_vault(dir: &tempfile::TempDir) -> Vault {
    let mut vault = vault_in(dir);
    vault.create("1234").unwrap();
    vault
}

fn import(vault: &mut Vault, nick: &str, key: &str) -> Result<IdentitySummary> {
    vault.add_identity(
        nick,
        NewKey::Import {
            key,
            password: None,
        },
    )
}

fn relays() -> Vec<String> {
    vec!["wss://relay.example".into()]
}

fn shared_document_of(vault: &Vault) -> SharedDocument {
    vault
        .unlocked()
        .unwrap()
        .data
        .backup
        .as_ref()
        .unwrap()
        .document
        .clone()
}

// Vault

#[test]
fn status_moves_from_no_vault_to_unlocked_to_locked() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault_in(&dir);
    assert_eq!(vault.status(), VaultStatus::NoVault);
    vault.create("1234").unwrap();
    assert_eq!(vault.status(), VaultStatus::Unlocked);
    vault.lock();
    assert_eq!(vault.status(), VaultStatus::Locked);
    vault.unlock("1234").unwrap();
    assert_eq!(vault.status(), VaultStatus::Unlocked);
}

#[test]
fn create_needs_four_characters_and_no_existing_vault() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault_in(&dir);
    assert!(matches!(vault.create("123"), Err(Error::PasswordTooShort)));
    assert_eq!(vault.status(), VaultStatus::NoVault);
    vault.create("1234").unwrap();
    assert!(matches!(vault.create("abcd"), Err(Error::VaultExists)));
}

#[test]
fn a_wrong_password_does_not_unlock() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault_in(&dir);
    vault.create("right").unwrap();
    vault.lock();
    assert!(matches!(vault.unlock("wrong"), Err(Error::InvalidPassword)));
    assert_eq!(vault.status(), VaultStatus::Locked);
}

#[test]
fn identities_survive_lock_and_a_new_process() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    let created = vault.add_identity("Alice", NewKey::Generate).unwrap();
    import(&mut vault, "Bob", HEX).unwrap();

    let mut reopened = vault_in(&dir);
    assert_eq!(reopened.status(), VaultStatus::Locked);
    reopened.unlock("1234").unwrap();
    let nicks: Vec<_> = reopened
        .identities()
        .unwrap()
        .into_iter()
        .map(|i| i.nick)
        .collect();
    assert_eq!(nicks, ["Alice", "Bob"]);
    assert_eq!(reopened.identity(&created.id).unwrap(), created);
}

#[test]
fn locked_vault_refuses_identity_and_sync_calls() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    vault.lock();
    assert!(matches!(vault.identities(), Err(Error::Locked)));
    assert!(matches!(
        vault.add_identity("A", NewKey::Generate),
        Err(Error::Locked)
    ));
    assert!(matches!(vault.backup_info(), Err(Error::Locked)));
}

#[test]
fn the_file_holds_no_private_key_or_nick_in_the_clear() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    import(&mut vault, "Very secret nick", HEX).unwrap();
    vault.enable_backup(PHRASE, &relays()).unwrap();
    let text = fs::read_to_string(vault.path()).unwrap();
    assert!(!text.contains(HEX));
    assert!(!text.contains("Very secret nick"));
    assert!(!text.contains("relay.example"));
    assert!(text.contains("gooti-desktop-vault"));
}

#[cfg(unix)]
#[test]
fn the_file_is_readable_only_by_its_owner() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let vault = unlocked_vault(&dir);
    let mode = fs::metadata(vault.path()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn a_duplicate_public_key_is_refused_with_the_existing_nick() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    import(&mut vault, "Alice", HEX).unwrap();
    let error = import(&mut vault, "Again", HEX).unwrap_err();
    assert_eq!(
        error.to_string(),
        "An identity with this public key already exists: Alice"
    );
}

#[test]
fn delete_removes_the_identity_and_allows_a_new_import() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    let alice = import(&mut vault, "Alice", HEX).unwrap();
    vault.delete_identity(&alice.id).unwrap();
    assert!(vault.identities().unwrap().is_empty());
    assert!(matches!(
        vault.reveal_nsec(&alice.id),
        Err(Error::IdentityNotFound)
    ));
    let again = import(&mut vault, "Alice again", HEX).unwrap();
    assert_eq!(vault.identities().unwrap(), vec![again]);
}

#[test]
fn rename_and_color_are_saved() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    let alice = import(&mut vault, "Alice", HEX).unwrap();
    vault.rename_identity(&alice.id, " Alicia ").unwrap();
    vault
        .set_identity_color(&alice.id, Some("#A1B2C3"))
        .unwrap();
    assert!(matches!(
        vault.rename_identity(&alice.id, " "),
        Err(Error::NickRequired)
    ));
    assert!(matches!(
        vault.set_identity_color(&alice.id, Some("red")),
        Err(Error::InvalidColor)
    ));

    vault.lock();
    vault.unlock("1234").unwrap();
    let saved = vault.identity(&alice.id).unwrap();
    assert_eq!(saved.nick, "Alicia");
    assert_eq!(saved.color.as_deref(), Some("#a1b2c3"));
    vault.set_identity_color(&alice.id, None).unwrap();
    assert_eq!(vault.identity(&alice.id).unwrap().color, None);
}

#[test]
fn reveal_and_export_return_the_identity_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    let alice = import(&mut vault, "Alice", HEX).unwrap();
    assert_eq!(
        vault.reveal_nsec(&alice.id).unwrap().as_str(),
        "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5"
    );
    let exported = vault.export_ncryptsec(&alice.id, "export").unwrap();
    let back = identity::keys_for(NewKey::Import {
        key: &exported,
        password: Some("export"),
    })
    .unwrap();
    assert_eq!(back.public_key().to_hex(), alice.pubkey);
}

#[test]
fn the_keychain_data_key_unlocks_without_the_password() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    import(&mut vault, "Alice", HEX).unwrap();
    let data_key = vault.data_key().unwrap();
    vault.lock();
    vault.unlock_with_data_key(&data_key).unwrap();
    assert_eq!(vault.identities().unwrap().len(), 1);
    vault.lock();
    assert!(matches!(
        vault.unlock_with_data_key(&[7u8; KEY_LEN]),
        Err(Error::InvalidPassword)
    ));
}

#[test]
fn change_password_needs_the_current_one_and_keeps_the_data_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault_in(&dir);
    vault.create("old-password").unwrap();
    import(&mut vault, "Alice", HEX).unwrap();
    let data_key = vault.data_key().unwrap();
    assert!(matches!(
        vault.change_password("wrong", "new-password"),
        Err(Error::InvalidPassword)
    ));
    assert!(matches!(
        vault.change_password("old-password", "abc"),
        Err(Error::PasswordTooShort)
    ));
    vault
        .change_password("old-password", "new-password")
        .unwrap();
    vault.lock();
    assert!(matches!(
        vault.unlock("old-password"),
        Err(Error::InvalidPassword)
    ));
    vault.unlock("new-password").unwrap();
    assert_eq!(*vault.data_key().unwrap(), *data_key);
    assert_eq!(vault.identities().unwrap().len(), 1);
}

#[test]
fn reset_deletes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    vault.reset().unwrap();
    assert_eq!(vault.status(), VaultStatus::NoVault);
    assert!(!vault.path().exists());
    vault.reset().unwrap();
}

#[test]
fn a_damaged_file_is_reported_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    fs::write(vault.path(), b"{ not json").unwrap();
    vault.lock();
    assert!(matches!(vault.unlock("1234"), Err(Error::UnreadableVault)));
    assert_eq!(fs::read(vault.path()).unwrap(), b"{ not json");
}

#[test]
fn a_crash_before_rename_keeps_the_old_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    import(&mut vault, "Alice", HEX).unwrap();
    fs::write(vault.path().with_extension("tmp"), b"half a file").unwrap();
    let mut reopened = vault_in(&dir);
    reopened.unlock("1234").unwrap();
    assert_eq!(reopened.identities().unwrap().len(), 1);
}

#[test]
fn rows_deleted_by_the_first_alpha_are_dropped_when_read() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    import(&mut vault, "Alice", HEX).unwrap();
    vault.unlocked_mut().unwrap().data.identities[0].deleted_at = Some(1);
    // `deleted_at` is not written any more; simulate an old file by sealing it by hand.
    let mut value = serde_json::to_value(&vault.unlocked().unwrap().data).unwrap();
    value["identities"][0]["deletedAt"] = 1.into();
    let unlocked = vault.unlocked().unwrap();
    let mut file = vault.read_file().unwrap();
    file.data = crypto::to_base64(&crypto::seal(
        &unlocked.data_key,
        &serde_json::to_vec(&value).unwrap(),
        AAD_DATA,
    ));
    write_atomic(vault.path(), &file_bytes(&file)).unwrap();
    vault.lock();
    vault.unlock("1234").unwrap();
    assert!(vault.identities().unwrap().is_empty());
}

// Backup

#[test]
fn relays_are_trimmed_deduplicated_and_must_be_wss() {
    let input: Vec<String> = [
        "  wss://a.example ",
        "",
        "wss://a.example",
        "WSS://b.example",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(
        normalize_relays(&input).unwrap(),
        ["wss://a.example", "WSS://b.example"]
    );
    assert!(matches!(
        normalize_relays(&["ws://plain".into()]),
        Err(Error::InvalidRelay)
    ));
    assert!(matches!(
        normalize_relays(&[" ".into()]),
        Err(Error::NoRelays)
    ));
}

#[test]
fn enable_sync_projects_the_identities_and_keeps_the_keys_sealed() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    import(&mut vault, "Alice", HEX).unwrap();
    assert_eq!(vault.backup_info().unwrap(), None);
    vault.enable_backup(PHRASE, &relays()).unwrap();
    assert!(matches!(
        vault.enable_backup(PHRASE, &relays()),
        Err(Error::BackupOn)
    ));

    let info = vault.backup_info().unwrap().unwrap();
    assert_eq!(info.relays, relays());
    assert!(!info.auto_backup);
    assert!(info.content_chars > 0);

    let document = shared_document_of(&vault);
    assert_eq!(document.identities.len(), 1);
    assert_eq!(document.identities[0].privkey.as_deref(), Some(HEX));
    // The extension always writes a color.
    assert!(document.identities[0].color.is_some());

    vault.lock();
    vault.unlock("1234").unwrap();
    assert_eq!(vault.backup_info().unwrap().unwrap().relays, relays());
}

#[test]
fn a_bad_passphrase_or_relay_leaves_sync_off() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    assert!(matches!(
        vault.enable_backup("hello world", &relays()),
        Err(Error::InvalidPassphrase)
    ));
    assert!(matches!(
        vault.enable_backup(PHRASE, &[]),
        Err(Error::NoRelays)
    ));
    assert_eq!(vault.backup_info().unwrap(), None);
}

#[test]
fn edits_stamp_the_sync_document_and_deletes_leave_a_tombstone() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    vault.enable_backup(PHRASE, &relays()).unwrap();
    let alice = import(&mut vault, "Alice", HEX).unwrap();
    let stamped = shared_document_of(&vault).identities[0].updated_at;
    std::thread::sleep(std::time::Duration::from_millis(3));

    vault.rename_identity(&alice.id, "Alicia").unwrap();
    let row = shared_document_of(&vault).identities[0].clone();
    assert_eq!(row.nick, "Alicia");
    assert!(row.updated_at > stamped);

    vault.delete_identity(&alice.id).unwrap();
    let row = shared_document_of(&vault).identities[0].clone();
    assert!(row.deleted_at.is_some());
    // Like the extension, the tombstone keeps the key, so the document stays valid there.
    assert_eq!(row.privkey.as_deref(), Some(HEX));
}

#[test]
fn finish_sync_applies_a_merge_unless_the_vault_changed_meanwhile() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    let alice = import(&mut vault, "Alice", HEX).unwrap();
    vault.enable_backup(PHRASE, &relays()).unwrap();

    // The relay copy renamed Alice later and added a bunker identity from a browser.
    let plan = vault.shared_plan().unwrap();
    let mut remote = plan.document.clone();
    remote.identities[0].nick = "Alice (phone)".into();
    remote.identities[0].updated_at += 1000.0;
    remote.identities.push(bunker_row());
    let merged = merge_shared_documents(&plan.document, &remote);

    // An edit while the relays were busy wins: nothing is applied, a new backup follows.
    vault
        .set_identity_color(&alice.id, Some("#123456"))
        .unwrap();
    assert_eq!(
        vault.finish_shared(plan.revision, merged.clone()).unwrap(),
        Applied::Stale
    );
    assert_eq!(vault.identity(&alice.id).unwrap().nick, "Alice");

    let plan = vault.shared_plan().unwrap();
    let merged = merge_shared_documents(&plan.document, &remote);
    assert_eq!(
        vault.finish_shared(plan.revision, merged).unwrap(),
        Applied::Done
    );
    let identities = vault.identities().unwrap();
    assert_eq!(identities.len(), 2);
    // Same public key, same id on this desktop.
    assert_eq!(identities[0].id, alice.id);
    assert_eq!(identities[0].nick, "Alice (phone)");
    assert!(identities[1].is_bunker);
    assert!(!identities[1].has_private_key);
    assert!(matches!(
        vault.reveal_nsec(&identities[1].id),
        Err(Error::NoPrivateKey)
    ));
}

#[test]
fn a_remote_delete_removes_the_identity_here() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    import(&mut vault, "Alice", HEX).unwrap();
    vault.enable_backup(PHRASE, &relays()).unwrap();
    let plan = vault.shared_plan().unwrap();
    let mut remote = plan.document.clone();
    remote.identities[0].deleted_at = Some(remote.identities[0].updated_at + 1.0);
    let merged = merge_shared_documents(&plan.document, &remote);
    assert_eq!(
        vault.finish_shared(plan.revision, merged).unwrap(),
        Applied::Done
    );
    assert!(vault.identities().unwrap().is_empty());
    // The tombstone stays, so the next backup does not bring it back.
    let plan = vault.shared_plan().unwrap();
    assert!(plan.document.identities[0].deleted_at.is_some());
}

#[test]
fn joining_merges_this_desktop_with_the_relay_copy() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    import(&mut vault, "Desktop only", HEX).unwrap();
    let (local, revision) = vault.join_plan().unwrap();
    let remote = SharedDocument {
        identities: vec![bunker_row()],
        ..SharedDocument::default()
    };
    let merged = merge_shared_documents(&local, &remote);
    let keys = derive_shared_keys(PHRASE).unwrap();
    assert_eq!(
        vault
            .finish_join(
                revision,
                &keys,
                &derive_desktop_keys(PHRASE).unwrap(),
                relays(),
                merged
            )
            .unwrap(),
        Applied::Done
    );
    assert_eq!(vault.identities().unwrap().len(), 2);
    assert!(vault.backup_info().unwrap().is_some());
    assert!(matches!(vault.join_plan(), Err(Error::BackupOn)));
}

#[test]
fn turning_sync_off_keeps_the_identities() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    import(&mut vault, "Alice", HEX).unwrap();
    vault.enable_backup(PHRASE, &relays()).unwrap();
    vault.set_auto_backup(true).unwrap();
    assert!(vault.auto_backup());
    vault.disable_backup().unwrap();
    assert_eq!(vault.backup_info().unwrap(), None);
    assert!(!vault.auto_backup());
    assert_eq!(vault.identities().unwrap().len(), 1);
    assert!(matches!(vault.shared_plan(), Err(Error::BackupOff)));
}

fn bunker_row() -> SharedIdentity {
    SharedIdentity {
        pubkey: "4f".repeat(32),
        privkey: None,
        nick: "Phone".into(),
        color: Some("#3a7bd5".into()),
        created_at: "2026-02-03T04:05:06.789Z".into(),
        updated_at: 5.0,
        deleted_at: None,
        bunker: Some(SharedBunker {
            pubkey: "55".repeat(32),
            relays: vec!["wss://relay.example".into()],
            secret: None,
            ask_site: Some(false),
            client_secret_key: Some("66".repeat(32)),
            extra: Default::default(),
        }),
        custody_at: None,
        extra: Default::default(),
    }
}

#[test]
fn a_shared_bunker_pointer_never_replaces_a_private_key_held_here() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    let alice = import(&mut vault, "Alice", HEX).unwrap();
    vault.enable_backup(PHRASE, &relays()).unwrap();

    // A browser linked Alice to a bunker later; its row has no private key and is newer.
    let plan = vault.shared_plan().unwrap();
    let mut remote = plan.document.clone();
    let mut row = bunker_row();
    row.pubkey = alice.pubkey.clone();
    row.nick = "Alice (bunker)".into();
    row.updated_at = remote.identities[0].updated_at + 1000.0;
    remote.identities = vec![row];
    let merged = merge_shared_documents(&plan.document, &remote);
    assert!(
        merged.identities[0].privkey.is_none(),
        "the browser's row wins the merge"
    );

    assert_eq!(
        vault.finish_shared(plan.revision, merged).unwrap(),
        Applied::Done
    );
    let kept = vault.identity(&alice.id).unwrap();
    assert!(kept.has_private_key);
    assert!(!kept.is_bunker);
    assert_eq!(kept.nick, "Alice (bunker)");
    assert_eq!(
        vault.reveal_nsec(&alice.id).unwrap().as_str(),
        "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5"
    );

    // The next projection is a valid row: a key and no bunker.
    let plan = vault.shared_plan().unwrap();
    let row = plan
        .document
        .identities
        .iter()
        .find(|i| i.pubkey == alice.pubkey)
        .unwrap();
    assert!(row.privkey.is_some() && row.bunker.is_none());
}

// Custody

fn row<'a>(document: &'a SharedDocument, pubkey: &str) -> &'a SharedIdentity {
    document
        .identities
        .iter()
        .find(|i| i.pubkey == pubkey)
        .unwrap()
}

/// The extension's own check (`assertSyncDocument`), ported in `parse_shared_document`.
fn valid_for_the_extension(document: &SharedDocument) -> bool {
    crate::backup::document::parse_shared_document(serde_json::to_value(document).unwrap())
        .is_some()
}

#[test]
fn a_desktop_only_identity_syncs_a_pointer_never_its_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    vault.enable_backup(PHRASE, &relays()).unwrap();
    let alice = vault
        .add_identity_as(
            "Alice",
            NewKey::Import {
                key: HEX,
                password: None,
            },
            Custody::Desktop,
        )
        .unwrap();
    assert_eq!(alice.custody, Custody::Desktop);
    assert!(alice.has_private_key);

    let document = vault.shared_plan().unwrap().document;
    let row = row(&document, &alice.pubkey);
    assert!(row.privkey.is_none());
    let pointer = row.bunker.as_ref().unwrap();
    let signer = vault.bunker_signer(&alice.pubkey).unwrap().unwrap();
    assert_eq!(pointer.pubkey, signer.public_key().to_hex());
    assert_eq!(pointer.relays, ["wss://relay.getgooti.com"]);
    assert_eq!(pointer.secret, None);
    assert!(pointer.client_secret_key.is_some());
    assert!(valid_for_the_extension(&document));
    assert!(!serde_json::to_string(&document).unwrap().contains(HEX));

    // Browsers reach it through one connection, Basic, without a link.
    let info = vault.bunker_info().unwrap();
    assert_eq!(info.connections.len(), 1);
    let browsers = &info.connections[0];
    assert!(browsers.browsers);
    assert_eq!(browsers.name.as_deref(), Some("Gooti browsers"));
    assert_eq!(browsers.trust, crate::bunker::policy::Trust::Basic);
    let client =
        nostr::key::SecretKey::from_hex(pointer.client_secret_key.as_ref().unwrap()).unwrap();
    assert_eq!(
        browsers.client_pubkey,
        nostr::key::Keys::new(client).public_key().to_hex()
    );
    assert!(matches!(
        vault.bunker_revoke(&browsers.id),
        Err(Error::BrowsersConnection)
    ));
    assert_eq!(
        vault.bunker_listen().signer_pubkeys,
        [signer.public_key().to_hex()]
    );
}

#[test]
fn desktop_only_needs_a_wss_bunker_relay() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    vault
        .bunker_set_relays(&["ws://127.0.0.1:7777".into()])
        .unwrap();
    let result = vault.add_identity_as("Alice", NewKey::Generate, Custody::Desktop);
    assert!(matches!(result, Err(Error::NoBrowserRelay)));
    assert!(vault.identities().unwrap().is_empty());

    vault
        .bunker_set_relays(&["wss://relay.example".into()])
        .unwrap();
    let alice = vault
        .add_identity_as("Alice", NewKey::Generate, Custody::Desktop)
        .unwrap();
    // The default relays may change freely: Alice's browsers keep theirs.
    vault
        .bunker_set_relays(&["ws://127.0.0.1:7777".into()])
        .unwrap();
    // Her browsers' own relays need a wss:// relay.
    let browsers = vault.bunker_info().unwrap().connections[0].clone();
    assert!(browsers.browsers && browsers.identity_id == alice.id);
    assert_eq!(browsers.relays, ["wss://relay.example"]);
    assert!(matches!(
        vault.bunker_set_app_relays(&browsers.id, &["ws://127.0.0.1:7777".into()]),
        Err(Error::NoBrowserRelay)
    ));
    // Chosen relays instead of the default, for a new identity and for a link.
    let bob = vault
        .add_identity_on(
            "Bob",
            NewKey::Generate,
            Custody::Desktop,
            Some(&["wss://bob.example".into()]),
        )
        .unwrap();
    let info = vault.bunker_info().unwrap();
    let bobs = info
        .connections
        .iter()
        .find(|c| c.identity_id == bob.id)
        .unwrap();
    assert_eq!(bobs.relays, ["wss://bob.example"]);
    assert!(matches!(
        vault.add_identity_on("Carol", NewKey::Generate, Custody::Desktop, Some(&[])),
        Err(Error::NoRelays)
    ));
}

#[test]
fn moving_the_key_to_the_desktop_and_back_stamps_a_newer_row() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    let alice = import(&mut vault, "Alice", HEX).unwrap();
    vault.enable_backup(PHRASE, &relays()).unwrap();
    let shared_row = row(&vault.shared_plan().unwrap().document, &alice.pubkey).clone();
    assert_eq!(shared_row.privkey.as_deref(), Some(HEX));

    std::thread::sleep(std::time::Duration::from_millis(3));
    let moved = vault.set_custody(&alice.id, Custody::Desktop).unwrap();
    assert_eq!(moved.custody, Custody::Desktop);
    let pointed = row(&vault.shared_plan().unwrap().document, &alice.pubkey).clone();
    assert!(pointed.privkey.is_none() && pointed.bunker.is_some());
    assert!(
        pointed.updated_at > shared_row.updated_at,
        "the pointer must win on every device"
    );
    assert!(pointed.custody_at.is_some() && shared_row.custody_at.is_none());
    // A browser that still has the key and edits its copy later does not undo the move.
    let edited_later = SharedIdentity {
        nick: "Edited later".into(),
        updated_at: pointed.updated_at + 60_000.0,
        ..shared_row.clone()
    };
    let merged = merge_shared_documents(
        &SharedDocument {
            identities: vec![edited_later],
            ..Default::default()
        },
        &SharedDocument {
            identities: vec![pointed.clone()],
            ..Default::default()
        },
    );
    assert!(merged.identities[0].privkey.is_none());
    // Browsers would merge: the newer pointer replaces their key.
    assert!(
        merge_shared_documents(
            &SharedDocument {
                identities: vec![shared_row.clone()],
                ..Default::default()
            },
            &SharedDocument {
                identities: vec![pointed.clone()],
                ..Default::default()
            },
        )
        .identities[0]
            .privkey
            .is_none()
    );

    std::thread::sleep(std::time::Duration::from_millis(3));
    let back = vault.set_custody(&alice.id, Custody::Shared).unwrap();
    assert_eq!(back.custody, Custody::Shared);
    let shared_again = row(&vault.shared_plan().unwrap().document, &alice.pubkey).clone();
    assert_eq!(shared_again.privkey.as_deref(), Some(HEX));
    assert!(shared_again.bunker.is_none());
    assert!(shared_again.updated_at > pointed.updated_at);
    assert!(shared_again.custody_at > pointed.custody_at);
    // And a stale pointer edited later does not take the key out of backup again.
    let stale_pointer = SharedIdentity {
        updated_at: shared_again.updated_at + 60_000.0,
        ..pointed.clone()
    };
    let merged = merge_shared_documents(
        &SharedDocument {
            identities: vec![shared_again.clone()],
            ..Default::default()
        },
        &SharedDocument {
            identities: vec![stale_pointer],
            ..Default::default()
        },
    );
    assert_eq!(merged.identities[0].privkey.as_deref(), Some(HEX));
    assert!(
        vault.bunker_info().unwrap().connections.is_empty(),
        "the browsers' session ends"
    );
}

#[test]
fn a_desktop_only_key_that_comes_back_is_replaced_by_its_pointer_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    vault.enable_backup(PHRASE, &relays()).unwrap();
    let alice = vault
        .add_identity_as(
            "Alice",
            NewKey::Import {
                key: HEX,
                password: None,
            },
            Custody::Desktop,
        )
        .unwrap();

    // A browser imports the key again and renames Alice: its row carries the key, the same
    // custody time, and is newer.
    let plan = vault.shared_plan().unwrap();
    let mut remote = plan.document.clone();
    let mut old = row(&remote, &alice.pubkey).clone();
    old.bunker = None;
    old.privkey = Some(HEX.into());
    old.nick = "Alice (renamed)".into();
    old.updated_at += 1000.0;
    remote.identities = vec![old.clone()];
    let merged = merge_shared_documents(&plan.document, &remote);
    assert_eq!(row(&merged, &alice.pubkey).privkey.as_deref(), Some(HEX));

    assert_eq!(
        vault.finish_shared(plan.revision, merged).unwrap(),
        Applied::SendAgain
    );
    let kept = vault.identity(&alice.id).unwrap();
    assert_eq!(kept.custody, Custody::Desktop);
    assert!(kept.has_private_key);
    assert_eq!(kept.nick, "Alice (renamed)");

    let again = row(&vault.shared_plan().unwrap().document, &alice.pubkey).clone();
    assert!(again.privkey.is_none() && again.bunker.is_some());
    assert!(again.updated_at > old.updated_at);
    assert_eq!(again.nick, "Alice (renamed)");
}

#[test]
fn a_custody_time_dropped_by_an_older_browser_is_sent_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    vault.enable_backup(PHRASE, &relays()).unwrap();
    let alice = vault
        .add_identity_as("Alice", NewKey::Generate, Custody::Desktop)
        .unwrap();
    let plan = vault.shared_plan().unwrap();
    let ours = row(&plan.document, &alice.pubkey).clone();
    assert!(ours.custody_at.is_some());

    // Gooti 2.0 renames Alice and publishes the pointer without custodyAt.
    let mut remote = plan.document.clone();
    let mut renamed = ours.clone();
    renamed.nick = "Alice (renamed)".into();
    renamed.custody_at = None;
    renamed.updated_at += 1000.0;
    remote.identities = vec![renamed.clone()];
    let merged = merge_shared_documents(&plan.document, &remote);
    assert_eq!(row(&merged, &alice.pubkey).custody_at, None);

    assert_eq!(
        vault.finish_shared(plan.revision, merged).unwrap(),
        Applied::SendAgain
    );
    let again = row(&vault.shared_plan().unwrap().document, &alice.pubkey).clone();
    assert_eq!(again.custody_at, ours.custody_at);
    assert_eq!(again.nick, "Alice (renamed)");
    assert!(again.updated_at > renamed.updated_at);
    assert!(valid_for_the_extension(
        &vault.shared_plan().unwrap().document
    ));

    // Once restored, the next backup has nothing to send again.
    let plan = vault.shared_plan().unwrap();
    let document = plan.document.clone();
    assert_eq!(
        vault.finish_shared(plan.revision, document).unwrap(),
        Applied::Done
    );
}

#[test]
fn new_bunker_relays_reach_the_pointer_with_a_newer_stamp() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    vault.enable_backup(PHRASE, &relays()).unwrap();
    let alice = vault
        .add_identity_as("Alice", NewKey::Generate, Custody::Desktop)
        .unwrap();
    let before = row(&vault.shared_plan().unwrap().document, &alice.pubkey).clone();
    std::thread::sleep(std::time::Duration::from_millis(3));
    // A new default changes nothing that exists.
    vault
        .bunker_set_relays(&["wss://default.example".into()])
        .unwrap();
    let same = row(&vault.shared_plan().unwrap().document, &alice.pubkey).clone();
    assert_eq!(same.bunker, before.bunker);
    // The browsers' own relays reach the pointer, with a newer stamp.
    let browsers = vault.bunker_info().unwrap().connections[0].id.clone();
    vault
        .bunker_set_app_relays(
            &browsers,
            &["wss://new.example".into(), "ws://127.0.0.1:7777".into()],
        )
        .unwrap();
    let after = row(&vault.shared_plan().unwrap().document, &alice.pubkey).clone();
    assert_eq!(after.bunker.as_ref().unwrap().relays, ["wss://new.example"]);
    assert!(after.updated_at > before.updated_at);
}

#[test]
fn deleting_a_desktop_only_identity_leaves_a_pointer_tombstone() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    vault.enable_backup(PHRASE, &relays()).unwrap();
    let alice = vault
        .add_identity_as(
            "Alice",
            NewKey::Import {
                key: HEX,
                password: None,
            },
            Custody::Desktop,
        )
        .unwrap();
    vault.delete_identity(&alice.id).unwrap();
    let document = vault.shared_plan().unwrap().document;
    let tombstone = row(&document, &alice.pubkey);
    assert!(tombstone.deleted_at.is_some());
    assert!(tombstone.privkey.is_none() && tombstone.bunker.is_some());
    assert!(valid_for_the_extension(&document));
    assert!(!serde_json::to_string(&document).unwrap().contains(HEX));
}

#[test]
fn custody_survives_a_sync_and_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    vault.enable_backup(PHRASE, &relays()).unwrap();
    let alice = vault
        .add_identity_as("Alice", NewKey::Generate, Custody::Desktop)
        .unwrap();
    let plan = vault.shared_plan().unwrap();
    assert_eq!(
        vault.finish_shared(plan.revision, plan.document).unwrap(),
        Applied::Done
    );
    vault.lock();
    vault.unlock("1234").unwrap();
    let kept = vault.identity(&alice.id).unwrap();
    assert_eq!(kept.custody, Custody::Desktop);
    assert!(kept.has_private_key);
    assert!(
        row(&vault.shared_plan().unwrap().document, &alice.pubkey)
            .privkey
            .is_none()
    );
}

#[test]
fn an_edit_wins_over_the_row_it_edits_even_when_that_row_is_from_the_future() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = unlocked_vault(&dir);
    let alice = import(&mut vault, "Alice", HEX).unwrap();
    vault.enable_backup(PHRASE, &relays()).unwrap();
    // A device with a clock an hour ahead renamed Alice.
    let plan = vault.shared_plan().unwrap();
    let mut remote = plan.document.clone();
    remote.identities[0].nick = "Ahead".into();
    remote.identities[0].updated_at += 3_600_000.0;
    let ahead = remote.identities[0].updated_at;
    let merged = merge_shared_documents(&plan.document, &remote);
    vault.finish_shared(plan.revision, merged).unwrap();

    vault.rename_identity(&alice.id, "Mine").unwrap();
    let row = vault.shared_plan().unwrap().document.identities[0].clone();
    assert_eq!(row.nick, "Mine");
    assert!(row.updated_at > ahead);
}
