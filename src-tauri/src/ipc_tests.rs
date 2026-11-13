//! The contract between the UI and the commands: each request body below is exactly what
//! `ui/src/app/core/core.service.ts` sends, including camelCase names and `null`s.
//! When a call there changes, change it here too.

use gooti_core::backup::relay::WebSocketRelays;
use gooti_core::{Gooti, KdfParams, MemoryKeychain};
use serde_json::{Value, json};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{App, Manager, WebviewWindow, WebviewWindowBuilder};

use crate::state::AppState;

struct Harness {
    _app: App<MockRuntime>,
    webview: WebviewWindow<MockRuntime>,
    _dir: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let gooti = Gooti::open_with_kdf(
            dir.path(),
            Box::new(MemoryKeychain::default()),
            KdfParams::TEST,
        );
        let app = mock_builder()
            .manage(AppState::new(
                gooti,
                WebSocketRelays {
                    wait: Some(std::time::Duration::from_millis(500)),
                },
            ))
            .invoke_handler(crate::invoke_handler())
            .build(mock_context(noop_assets()))
            .unwrap();
        let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        Harness {
            _app: app,
            webview,
            _dir: dir,
        }
    }

    fn call(&self, cmd: &str, body: Value) -> Result<Value, Value> {
        get_ipc_response(
            &self.webview,
            InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: APP_URL.parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map(|body| body.deserialize::<Value>().unwrap())
    }

    fn ok(&self, cmd: &str, body: Value) -> Value {
        self.call(cmd, body)
            .unwrap_or_else(|error| panic!("{cmd} failed: {error}"))
    }

    fn err(&self, cmd: &str, body: Value) -> Value {
        self.call(cmd, body)
            .err()
            .unwrap_or_else(|| panic!("{cmd} should have failed"))
    }
}

/// Where the app's own pages come from. Only this origin may call the commands.
const APP_URL: &str = if cfg!(windows) {
    "http://tauri.localhost"
} else {
    "tauri://localhost"
};

const NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

#[test]
fn vault_lifecycle_and_errors_reach_the_ui_as_code_and_message() {
    let h = Harness::new();
    assert_eq!(h.ok("vault_status", json!({})), "noVault");

    let short = h.err("vault_create", json!({ "password": "123" }));
    assert_eq!(short["code"], "password_too_short");
    assert_eq!(short["message"], "Use at least 4 characters.");

    h.ok("vault_create", json!({ "password": "1234" }));
    assert_eq!(h.ok("vault_status", json!({})), "unlocked");

    h.ok("vault_lock", json!({}));
    assert_eq!(h.ok("vault_status", json!({})), "locked");
    assert_eq!(h.err("identities_list", json!({}))["code"], "locked");

    let wrong = h.err("vault_unlock", json!({ "password": "nope" }));
    assert_eq!(wrong["code"], "invalid_password");
    h.ok("vault_unlock", json!({ "password": "1234" }));

    h.ok(
        "vault_change_password",
        json!({ "current": "1234", "next": "5678" }),
    );
    h.ok("vault_lock", json!({}));
    h.ok("vault_unlock", json!({ "password": "5678" }));

    h.ok("vault_reset", json!({}));
    assert_eq!(h.ok("vault_status", json!({})), "noVault");
}

#[test]
fn identity_calls_use_the_ui_argument_names() {
    let h = Harness::new();
    h.ok("vault_create", json!({ "password": "1234" }));

    // Generate: `key` and `keyPassword` are null.
    let generated = h.ok(
        "identity_create",
        json!({ "nick": "Alice", "key": null, "keyPassword": null }),
    );
    assert_eq!(generated["nick"], "Alice");
    assert_eq!(generated["hasPrivateKey"], true);
    assert!(generated["npub"].as_str().unwrap().starts_with("npub1"));
    assert!(generated.get("privkey").is_none());

    // Import an nsec.
    let imported = h.ok(
        "identity_create",
        json!({ "nick": "Bob", "key": NSEC, "keyPassword": null }),
    );
    let id = imported["id"].as_str().unwrap().to_string();

    // Import an ncryptsec: the password travels as `keyPassword`.
    let ncryptsec = "ncryptsec1qgg9947rlpvqu76pj5ecreduf9jxhselq2nae2kghhvd5g7dgjtcxfqtd67p9m0w57lspw8gsq6yphnm8623nsl8xn9j4jdzz84zm3frztj3z7s35vpzmqf6ksu8r89qk5z2zxfmu5gv8th8wclt0h4p";
    let missing = h.err(
        "identity_create",
        json!({ "nick": "Carol", "key": ncryptsec, "keyPassword": null }),
    );
    assert_eq!(missing["code"], "key_password_required");
    h.ok(
        "identity_create",
        json!({ "nick": "Carol", "key": ncryptsec, "keyPassword": "nostr" }),
    );

    let duplicate = h.err(
        "identity_create",
        json!({ "nick": "Again", "key": NSEC, "keyPassword": null }),
    );
    assert_eq!(
        duplicate["message"],
        "An identity with this public key already exists: Bob"
    );

    let list = h.ok("identities_list", json!({}));
    assert_eq!(list.as_array().unwrap().len(), 3);

    assert_eq!(h.ok("identity_get", json!({ "id": id }))["nick"], "Bob");
    assert_eq!(
        h.ok("identity_rename", json!({ "id": id, "nick": "Robert" }))["nick"],
        "Robert"
    );
    assert_eq!(
        h.ok(
            "identity_set_color",
            json!({ "id": id, "color": "#AABBCC" })
        )["color"],
        "#aabbcc"
    );
    assert_eq!(
        h.ok("identity_set_color", json!({ "id": id, "color": null }))["color"],
        Value::Null
    );
    assert_eq!(h.ok("identity_reveal_nsec", json!({ "id": id })), NSEC);
    let exported = h.ok(
        "identity_export_ncryptsec",
        json!({ "id": id, "password": "export" }),
    );
    assert!(exported.as_str().unwrap().starts_with("ncryptsec1"));

    h.ok("identity_delete", json!({ "id": id }));
    assert_eq!(
        h.err("identity_get", json!({ "id": id }))["code"],
        "identity_not_found"
    );
}

#[test]
fn settings_calls_use_the_ui_argument_names() {
    let h = Harness::new();
    let settings = h.ok("settings_get", json!({}));
    assert_eq!(
        settings,
        json!({
            "unlockAtLogin": false,
            "autoLockMinutes": null,
            "lockOnScreenLock": true,
            "lockOnSleep": true,
            "keepRunning": true,
            "checkUpdates": true
        })
    );

    // Turning on "Unlock when Gooti starts" needs an unlocked vault.
    assert_eq!(
        h.err("settings_set_unlock_at_login", json!({ "on": true }))["code"],
        "locked"
    );
    h.ok("vault_create", json!({ "password": "1234" }));
    assert_eq!(
        h.ok("settings_set_unlock_at_login", json!({ "on": true }))["unlockAtLogin"],
        true
    );

    let changed = h.ok(
        "settings_change_lock",
        json!({ "change": { "autoLockMinutes": 15 } }),
    );
    assert_eq!(changed["autoLockMinutes"], 15);
    assert_eq!(changed["lockOnSleep"], true);

    let off = h.ok(
        "settings_change_lock",
        json!({ "change": { "autoLockMinutes": null, "lockOnSleep": false } }),
    );
    assert_eq!(off["autoLockMinutes"], Value::Null);
    assert_eq!(off["lockOnSleep"], false);
}

const PHRASE: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
/// Nothing listens on port 9, so a backup fails at once with that relay's reason.
const DEAD_RELAY: &str = "wss://127.0.0.1:9";
/// The same for the bunker, which also takes `ws://` on this computer.
const DEAD_LOCAL_RELAY: &str = "ws://127.0.0.1:9";

#[test]
fn backup_settings_calls_use_the_ui_argument_names() {
    let h = Harness::new();
    assert_eq!(
        h.ok("backup_default_relays", json!({})),
        json!(["wss://relay.getgooti.com"])
    );
    let phrase = h.ok("backup_generate_passphrase", json!({}));
    assert_eq!(phrase.as_str().unwrap().split(' ').count(), 12);
    h.ok("backup_check_passphrase", json!({ "phrase": phrase }));
    let invalid = h.err(
        "backup_check_passphrase",
        json!({ "phrase": "hello world" }),
    );
    assert_eq!(invalid["code"], "invalid_passphrase");
    assert_eq!(
        invalid["message"],
        "That passphrase is not valid. Check your 12 words."
    );

    h.ok("vault_create", json!({ "password": "1234" }));
    assert_eq!(h.ok("backup_info", json!({})), Value::Null);
    assert_eq!(h.ok("backup_busy", json!({})), json!(false));
    assert_eq!(h.err("backup_now", json!({}))["code"], "backup_off");

    let bad_relay = h.err(
        "backup_enable",
        json!({ "phrase": PHRASE, "relays": ["ws://plain"] }),
    );
    assert_eq!(bad_relay["message"], "Relays must be wss:// URLs.");

    let info = h.ok(
        "backup_enable",
        json!({ "phrase": PHRASE, "relays": [" wss://relay.example ", ""] }),
    );
    assert_eq!(info["relays"], json!(["wss://relay.example"]));
    assert_eq!(info["autoBackup"], false);
    assert!(info["contentChars"].as_u64().unwrap() > 0);
    // No identities yet: nothing that only this desktop holds.
    assert_eq!(info["desktopCurrent"], true);

    let info = h.ok(
        "backup_set_relays",
        json!({ "relays": ["wss://a.example", "wss://b.example"] }),
    );
    assert_eq!(
        info["relays"],
        json!(["wss://a.example", "wss://b.example"])
    );
    assert_eq!(
        h.ok("backup_set_auto", json!({ "on": false }))["autoBackup"],
        false
    );

    h.ok("backup_disable", json!({}));
    assert_eq!(h.ok("backup_info", json!({})), Value::Null);
}

#[test]
fn a_failed_sync_lists_each_relay_and_its_reason() {
    let h = Harness::new();
    h.ok("vault_create", json!({ "password": "1234" }));
    h.ok(
        "backup_enable",
        json!({ "phrase": PHRASE, "relays": [DEAD_RELAY] }),
    );
    let error = h.err("backup_now", json!({}));
    assert_eq!(error["code"], "relays_failed");
    assert_eq!(error["message"], "No relay could be reached.");
    assert_eq!(error["failures"][0]["url"], DEAD_RELAY);
    assert!(!error["failures"][0]["reason"].as_str().unwrap().is_empty());
}

#[test]
fn restore_and_join_report_relay_failures_and_store_nothing() {
    let h = Harness::new();
    let error = h.err(
        "vault_restore",
        json!({ "password": "1234", "phrase": PHRASE, "relays": [DEAD_RELAY] }),
    );
    assert_eq!(error["code"], "relays_failed");
    assert_eq!(h.ok("vault_status", json!({})), "noVault");

    h.ok("vault_create", json!({ "password": "1234" }));
    let error = h.err(
        "backup_join",
        json!({ "phrase": PHRASE, "relays": [DEAD_RELAY] }),
    );
    assert_eq!(error["code"], "relays_failed");
    assert_eq!(h.ok("backup_info", json!({})), Value::Null);
}

#[test]
fn bunker_calls_use_the_ui_argument_names() {
    let h = Harness::new();
    h.ok("vault_create", json!({ "password": "1234" }));
    let alice = h.ok(
        "identity_create",
        json!({ "nick": "Alice", "key": null, "keyPassword": null }),
    );
    let id = alice["id"].as_str().unwrap();

    let info = h.ok("bunker_info", json!({}));
    assert_eq!(info["relays"], json!(["wss://relay.getgooti.com"]));
    assert_eq!(info["paused"], false);
    assert_eq!(info["connections"], json!([]));
    assert_eq!(info["relayStatus"], json!([]));

    // Keep the tests off the internet: a local relay that does not answer.
    assert_eq!(
        h.ok("bunker_set_relays", json!({ "relays": [DEAD_LOCAL_RELAY] })),
        json!([DEAD_LOCAL_RELAY])
    );
    let bad = h.err(
        "bunker_set_relays",
        json!({ "relays": ["ws://relay.example"] }),
    );
    assert_eq!(bad["code"], "invalid_relay");

    let link = h.ok(
        "bunker_link",
        json!({ "identityId": id, "relays": [DEAD_LOCAL_RELAY] }),
    );
    assert!(link.as_str().unwrap().starts_with("bunker://"));
    assert_eq!(
        h.ok("bunker_info", json!({}))["linkRelays"],
        json!([DEAD_LOCAL_RELAY])
    );
    let status = h.ok("bunker_info", json!({}))["relayStatus"].clone();
    assert_eq!(status[0]["url"], DEAD_LOCAL_RELAY);

    let app_key = "eff37350d839ce3707332348af4549a96051bd695d3223af4aabce4993531d86";
    h.ok(
        "bunker_connect",
        json!({
            "link": format!("nostrconnect://{app_key}?relay={DEAD_LOCAL_RELAY}&secret=x&name=Snort"),
            "identityId": id
        }),
    );
    let bad_link = h.err(
        "bunker_connect",
        json!({ "link": "hello", "identityId": id }),
    );
    assert_eq!(bad_link["message"], "This is not a nostrconnect:// link.");

    let info = h.ok("bunker_info", json!({}));
    let connection = &info["connections"][0];
    assert_eq!(connection["name"], "Snort");
    assert_eq!(connection["identityNick"], "Alice");
    assert_eq!(connection["identityId"], id);

    assert_eq!(h.ok("bunker_pending", json!({})), json!([]));
    assert_eq!(
        h.ok(
            "bunker_decide",
            json!({ "key": "nothing", "approve": true, "remember": "oneHour" })
        ),
        false
    );
    let bad_remember = h.err(
        "bunker_decide",
        json!({ "key": "nothing", "approve": true, "remember": "forever" }),
    );
    assert!(
        bad_remember.to_string().contains("remember"),
        "{bad_remember}"
    );

    // One app's settings: trust level and remembered choices.
    let connection_id = connection["id"].as_str().unwrap();
    let detail = h.ok(
        "bunker_connection",
        json!({ "connectionId": connection_id }),
    );
    assert_eq!(detail["trust"], "ask");
    assert_eq!(detail["rules"], json!([]));
    assert_eq!(detail["name"], "Snort");
    let detail = h.ok(
        "bunker_set_trust",
        json!({ "connectionId": connection_id, "trust": "basic" }),
    );
    assert_eq!(detail["trust"], "basic");
    let detail = h.ok(
        "bunker_forget",
        json!({ "connectionId": connection_id, "method": "sign_event", "kind": 1 }),
    );
    assert_eq!(detail["rules"], json!([]));

    // Connecting the app is in Activity.
    let activity = h.ok("activity_list", json!({}));
    assert_eq!(activity[0]["type"], "connected");
    assert_eq!(activity[0]["app"], "Snort");
    h.ok("activity_clear", json!({}));
    assert_eq!(h.ok("activity_list", json!({})), json!([]));

    h.ok("bunker_set_paused", json!({ "paused": true }));
    assert_eq!(h.ok("bunker_info", json!({}))["paused"], true);
    h.ok(
        "bunker_revoke",
        json!({ "connectionId": connection["id"].as_str().unwrap() }),
    );
    assert_eq!(h.ok("bunker_info", json!({}))["connections"], json!([]));

    // Locking stops the bunker.
    h.ok("vault_lock", json!({}));
    assert_eq!(
        h.err("bunker_link", json!({ "identityId": id }))["code"],
        "locked"
    );
    assert_eq!(h.ok("bunker_pending", json!({})), json!([]));
}

#[test]
fn custody_calls_use_the_ui_argument_names() {
    let h = Harness::new();
    h.ok("vault_create", json!({ "password": "1234" }));
    let alice = h.ok(
        "identity_create",
        json!({
            "nick": "Alice",
            "key": null,
            "keyPassword": null,
            "custody": "desktop",
            "relays": ["wss://alice.example"]
        }),
    );
    assert_eq!(alice["custody"], "desktop");
    let id = alice["id"].as_str().unwrap();

    let connections = h.ok("bunker_info", json!({}))["connections"].clone();
    assert_eq!(connections[0]["name"], "Gooti browsers");
    assert_eq!(connections[0]["relays"], json!(["wss://alice.example"]));
    let browsers_id = connections[0]["id"].as_str().unwrap().to_string();
    assert_eq!(
        h.ok(
            "bunker_set_app_relays",
            json!({ "connectionId": browsers_id, "relays": ["wss://other.example"] }),
        ),
        json!(["wss://other.example"])
    );
    assert_eq!(connections[0]["browsers"], true);
    assert_eq!(connections[0]["trust"], "basic");
    let refused = h.err(
        "bunker_revoke",
        json!({ "connectionId": connections[0]["id"].as_str().unwrap() }),
    );
    assert_eq!(refused["code"], "browsers_connection");

    let shared = h.ok(
        "identity_set_custody",
        json!({ "id": id, "custody": "shared", "relays": null }),
    );
    assert_eq!(shared["custody"], "shared");
    assert_eq!(h.ok("bunker_info", json!({}))["connections"], json!([]));
    assert_eq!(shared["servedElsewhere"], false);
    assert_eq!(shared["keyMissing"], false);

    let served = h.ok("identity_serve_here", json!({ "ids": [id] }));
    assert_eq!(served[0]["servedElsewhere"], false);

    // Without custody the identity is shared, as before.
    let bob = h.ok(
        "identity_create",
        json!({ "nick": "Bob", "key": null, "keyPassword": null }),
    );
    assert_eq!(bob["custody"], "shared");
}

#[test]
fn app_settings_use_the_ui_argument_names() {
    let h = Harness::new();
    let settings = h.ok("settings_get", json!({}));
    assert_eq!(settings["keepRunning"], true);
    assert_eq!(settings["checkUpdates"], true);
    let changed = h.ok(
        "settings_change_app",
        json!({ "change": { "keepRunning": false } }),
    );
    assert_eq!(changed["keepRunning"], false);
    assert_eq!(changed["checkUpdates"], true);
    let changed = h.ok(
        "settings_change_app",
        json!({ "change": { "checkUpdates": false } }),
    );
    assert_eq!(changed["keepRunning"], false);
    assert_eq!(changed["checkUpdates"], false);
}

#[test]
fn a_clicked_link_waits_for_the_unlock_and_is_taken_once() {
    let h = Harness::new();
    let state = h._app.state::<AppState>();
    state.set_connect_link("nostrconnect://abc?secret=x".into());
    assert_eq!(
        h.ok("connect_link_take", json!({})),
        Value::Null,
        "no vault yet"
    );
    h.ok("vault_create", json!({ "password": "1234" }));
    assert_eq!(
        h.ok("connect_link_take", json!({})),
        json!("nostrconnect://abc?secret=x")
    );
    assert_eq!(h.ok("connect_link_take", json!({})), Value::Null);
}
