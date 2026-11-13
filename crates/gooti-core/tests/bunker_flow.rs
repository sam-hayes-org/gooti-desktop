//! The bunker over a real WebSocket relay on localhost, with a small NIP-46 app as the client.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Mode, start_relay};
use futures_util::{SinkExt, StreamExt};
use gooti_core::bunker::message::{Scheme, open_response, request_event};
use gooti_core::bunker::policy::{Remember, Trust};
use gooti_core::bunker::pool::{RelayState, RelayStatus};
use gooti_core::bunker::service::{BunkerService, Notice};
use gooti_core::{Gooti, KdfParams, MemoryKeychain, NewKey};
use nostr::event::{Event, FinalizeEvent};
use nostr::key::Keys;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

/// A NIP-46 app: listens for answers to its key and sends requests.
struct App {
    keys: Keys,
    signer: String,
    socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    next: u32,
}

impl App {
    async fn connect(relay: &str, signer: &str) -> App {
        let keys = Keys::generate();
        let (mut socket, _) = connect_async(relay).await.unwrap();
        let filter = json!({ "kinds": [24133], "#p": [keys.public_key().to_hex()] });
        socket
            .send(Message::text(json!(["REQ", "app", filter]).to_string()))
            .await
            .unwrap();
        App {
            keys,
            signer: signer.to_string(),
            socket,
            next: 0,
        }
    }

    /// Sends a request and waits for the answer with the same id.
    async fn call(&mut self, method: &str, params: &[&str]) -> Value {
        self.next += 1;
        let id = self.next.to_string();
        let event = request_event(&self.keys, &self.signer, &id, method, params, Scheme::Nip44);
        self.socket
            .send(Message::text(json!(["EVENT", event]).to_string()))
            .await
            .unwrap();
        self.answer(&id).await
    }

    async fn answer(&mut self, id: &str) -> Value {
        let wait = async {
            while let Some(Ok(message)) = self.socket.next().await {
                // A real relay also sends pings and other frames.
                let Message::Text(text) = message else {
                    continue;
                };
                let frame: Vec<Value> = serde_json::from_str(text.as_str()).unwrap();
                if frame[0] == "EVENT" {
                    let event: Event = serde_json::from_value(frame[2].clone()).unwrap();
                    if let Some(answer) = open_response(&event, &self.keys)
                        && answer["id"] == id
                    {
                        return answer;
                    }
                }
            }
            panic!("the relay closed");
        };
        tokio::time::timeout(Duration::from_secs(10), wait)
            .await
            .expect("the bunker did not answer")
    }

    /// Sends a request without waiting.
    async fn send(&mut self, method: &str, params: &[&str]) -> String {
        self.next += 1;
        let id = self.next.to_string();
        let event = request_event(&self.keys, &self.signer, &id, method, params, Scheme::Nip44);
        self.socket
            .send(Message::text(json!(["EVENT", event]).to_string()))
            .await
            .unwrap();
        id
    }
}

struct Desktop {
    gooti: Arc<Mutex<Gooti>>,
    service: Arc<BunkerService>,
    notices: mpsc::UnboundedReceiver<Notice>,
    identity_id: String,
    identity_pubkey: String,
    _dir: tempfile::TempDir,
}

impl Desktop {
    fn start(relay: &str) -> Desktop {
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
            .bunker_set_relays(&[relay.to_string()])
            .unwrap();
        let gooti = Arc::new(Mutex::new(gooti));
        let (tx, notices) = mpsc::unbounded_channel();
        let service = BunkerService::start(
            Arc::clone(&gooti),
            Arc::new(move |notice| {
                let _ = tx.send(notice);
            }),
        );
        Desktop {
            gooti,
            service,
            notices,
            identity_id: alice.id,
            identity_pubkey: alice.pubkey,
            _dir: dir,
        }
    }

    /// A fresh `bunker://` link: its signer key and secret.
    fn link(&self) -> (String, String) {
        self.link_for(&self.identity_id)
    }

    fn link_for(&self, identity_id: &str) -> (String, String) {
        let link = self
            .gooti
            .lock()
            .unwrap()
            .vault_mut()
            .bunker_link(identity_id)
            .unwrap();
        self.service.refresh();
        let signer = link.trim_start_matches("bunker://")[..64].to_string();
        let secret = link.split("secret=").nth(1).unwrap();
        let secret = secret.split('&').next().unwrap().to_string();
        (signer, secret)
    }

    async fn next_ask(&mut self) -> gooti_core::bunker::handler::Ask {
        loop {
            let notice = tokio::time::timeout(Duration::from_secs(10), self.notices.recv())
                .await
                .expect("no request arrived")
                .unwrap();
            if let Notice::Asked { ask } = notice {
                return *ask;
            }
        }
    }
}

impl Desktop {
    /// Waits for a `Relays` notice after which `ready` holds for the relay status.
    async fn relays_until(&mut self, ready: impl Fn(&[RelayStatus]) -> bool) {
        let wait = async {
            loop {
                if let Notice::Relays = self.notices.recv().await.unwrap()
                    && ready(&self.service.relay_status())
                {
                    return;
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(10), wait)
            .await
            .expect("no Relays notice with that status");
    }
}

/// The pool connects in the background; give it a moment before the app talks.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_app_connects_signs_encrypts_and_logs_out() {
    let (relay, _) = start_relay(Mode::Normal).await;
    let mut desktop = Desktop::start(&relay);
    let (signer, secret) = desktop.link();
    settle().await;
    assert!(
        desktop
            .service
            .relay_status()
            .iter()
            .all(|s| s.state == gooti_core::bunker::pool::RelayState::Connected)
    );

    let mut app = App::connect(&relay, &signer).await;
    let meta = r#"{"name":"Test app"}"#;
    assert_eq!(
        app.call("connect", &[&signer, &secret, "", meta]).await["result"],
        "ack"
    );
    assert_eq!(
        app.call("get_public_key", &[]).await["result"],
        desktop.identity_pubkey
    );
    assert_eq!(app.call("ping", &[]).await["result"], "pong");
    let relays: Vec<String> = serde_json::from_str(
        app.call("switch_relays", &[]).await["result"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(relays, std::slice::from_ref(&relay));
    let info = desktop.gooti.lock().unwrap().vault().bunker_info().unwrap();
    assert_eq!(info.connections[0].name.as_deref(), Some("Test app"));

    // Signing waits for the user.
    let template =
        r#"{"kind":1,"content":"hello from the bunker","tags":[],"created_at":1700000000}"#;
    let id = app.send("sign_event", &[template]).await;
    let ask = desktop.next_ask().await;
    assert_eq!(
        ask.details.content.as_deref(),
        Some("hello from the bunker")
    );
    assert_eq!(desktop.service.pending().len(), 1);
    assert!(
        desktop
            .service
            .decide(&ask.key, true, Remember::Once)
            .unwrap()
    );
    let answer = app.answer(&id).await;
    let signed: Event = serde_json::from_str(answer["result"].as_str().unwrap()).unwrap();
    signed.verify().unwrap();
    assert_eq!(signed.pubkey.to_hex(), desktop.identity_pubkey);
    assert!(desktop.service.pending().is_empty());

    // Rejected.
    let id = app.send("sign_event", &[template]).await;
    let ask = desktop.next_ask().await;
    desktop
        .service
        .decide(&ask.key, false, Remember::Once)
        .unwrap();
    assert_eq!(app.answer(&id).await["error"], "Rejected.");
    assert!(
        !desktop
            .service
            .decide(&ask.key, true, Remember::Once)
            .unwrap(),
        "an answered request is gone"
    );

    // Encrypting for someone else.
    let peer = Keys::generate();
    let peer_hex = peer.public_key().to_hex();
    let id = app.send("nip44_encrypt", &[&peer_hex, "secret note"]).await;
    let ask = desktop.next_ask().await;
    desktop
        .service
        .decide(&ask.key, true, Remember::Once)
        .unwrap();
    let ciphertext = app.answer(&id).await["result"]
        .as_str()
        .unwrap()
        .to_string();
    let plain = nostr::nips::nip44::decrypt(
        peer.secret_key(),
        &nostr::key::PublicKey::from_hex(&desktop.identity_pubkey).unwrap(),
        &ciphertext,
    )
    .unwrap();
    assert_eq!(plain, "secret note");

    assert_eq!(app.call("logout", &[]).await["result"], "ack");
    assert!(
        desktop
            .gooti
            .lock()
            .unwrap()
            .vault()
            .bunker_info()
            .unwrap()
            .connections
            .is_empty()
    );
    // With no app and no open link left, the bunker stops listening for this identity.
    settle().await;
    assert!(desktop.service.relay_status().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_app_without_a_connection_gets_an_error() {
    let (relay, _) = start_relay(Mode::Normal).await;
    let desktop = Desktop::start(&relay);
    let (signer, _) = desktop.link();
    settle().await;
    let mut app = App::connect(&relay, &signer).await;
    let answer = app.call("get_public_key", &[]).await;
    assert_eq!(
        answer["error"],
        "Not connected. Connect to this bunker first."
    );
    let answer = app.call("connect", &[&signer, "wrong secret"]).await;
    assert_eq!(
        answer["error"],
        "This connection link was used before or has expired."
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_relay_that_wants_auth_gets_it() {
    let (relay, _) = start_relay(Mode::Auth).await;
    let desktop = Desktop::start(&relay);
    let (signer, secret) = desktop.link();
    settle().await;
    let mut app = App::connect(&relay, &signer).await;
    // The app's own subscription needs AUTH on this relay too.
    let auth = {
        let tags = [
            nostr::event::Tag::parse(["relay", relay.as_str()]).unwrap(),
            nostr::event::Tag::parse(["challenge", "challenge-123"]).unwrap(),
        ];
        nostr::event::EventBuilder::new(nostr::event::Kind::Authentication, "")
            .tags(tags)
            .finalize(&app.keys)
            .unwrap()
    };
    app.socket
        .send(Message::text(json!(["AUTH", auth]).to_string()))
        .await
        .unwrap();
    let filter = json!({ "kinds": [24133], "#p": [app.keys.public_key().to_hex()] });
    app.socket
        .send(Message::text(json!(["REQ", "app", filter]).to_string()))
        .await
        .unwrap();
    assert_eq!(
        app.call("connect", &[&signer, &secret]).await["result"],
        "ack"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn several_identities_on_a_strict_relay_each_answer() {
    // relay.getgooti.com refuses a filter with more than one #p value, so every signer key
    // needs its own subscription.
    let (relay, _) = start_relay(Mode::Strict).await;
    let desktop = Desktop::start(&relay);
    let (bob_id, bob_pubkey, carol_id) = {
        let mut gooti = desktop.gooti.lock().unwrap();
        let bob = gooti
            .vault_mut()
            .add_identity("Bob", NewKey::Generate)
            .unwrap();
        let carol = gooti
            .vault_mut()
            .add_identity("Carol", NewKey::Generate)
            .unwrap();
        (bob.id, bob.pubkey, carol.id)
    };
    let (alice_signer, alice_secret) = desktop.link();
    let (bob_signer, bob_secret) = desktop.link_for(&bob_id);
    let (carol_signer, _) = desktop.link_for(&carol_id);
    assert_eq!(
        [&alice_signer, &bob_signer, &carol_signer]
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3
    );
    settle().await;

    let mut alice_app = App::connect(&relay, &alice_signer).await;
    let mut bob_app = App::connect(&relay, &bob_signer).await;
    assert_eq!(
        alice_app
            .call("connect", &[&alice_signer, &alice_secret])
            .await["result"],
        "ack"
    );
    assert_eq!(
        bob_app.call("connect", &[&bob_signer, &bob_secret]).await["result"],
        "ack"
    );
    assert_eq!(
        alice_app.call("get_public_key", &[]).await["result"],
        desktop.identity_pubkey
    );
    assert_eq!(
        bob_app.call("get_public_key", &[]).await["result"],
        bob_pubkey
    );

    // A key that goes away closes its subscription; the others keep answering.
    desktop
        .gooti
        .lock()
        .unwrap()
        .vault_mut()
        .delete_identity(&carol_id)
        .unwrap();
    desktop.service.refresh();
    settle().await;
    assert_eq!(alice_app.call("ping", &[]).await["result"], "pong");
    assert_eq!(bob_app.call("ping", &[]).await["result"], "pong");

    // One app logging out leaves the other connected.
    assert_eq!(alice_app.call("logout", &[]).await["result"], "ack");
    settle().await;
    assert_eq!(bob_app.call("ping", &[]).await["result"], "pong");
    assert_eq!(bob_app.call("logout", &[]).await["result"], "ack");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pasted_nostrconnect_link_answers_with_its_secret() {
    let (relay, _) = start_relay(Mode::Normal).await;
    let desktop = Desktop::start(&relay);
    let mut app = App::connect(&relay, &"00".repeat(32)).await;
    let link = format!(
        "nostrconnect://{}?relay={}&secret=sesame&name=Snort",
        app.keys.public_key().to_hex(),
        relay
    );
    desktop
        .service
        .accept_nostrconnect(&link, &desktop.identity_id)
        .unwrap();

    let wait = async {
        while let Some(Ok(Message::Text(text))) = app.socket.next().await {
            let frame: Vec<Value> = serde_json::from_str(text.as_str()).unwrap();
            if frame[0] == "EVENT" {
                let event: Event = serde_json::from_value(frame[2].clone()).unwrap();
                if let Some(answer) = open_response(&event, &app.keys) {
                    return (event.pubkey.to_hex(), answer);
                }
            }
        }
        panic!("the relay closed");
    };
    let (signer, answer) = tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .unwrap();
    assert_eq!(answer["result"], "sesame");
    app.signer = signer;
    assert_eq!(
        app.call("get_public_key", &[]).await["result"],
        desktop.identity_pubkey
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn remembered_choices_and_trust_levels_answer_without_asking_and_are_logged() {
    let (relay, _) = start_relay(Mode::Normal).await;
    let mut desktop = Desktop::start(&relay);
    let (signer, secret) = desktop.link();
    settle().await;
    let mut app = App::connect(&relay, &signer).await;
    app.call(
        "connect",
        &[&signer, &secret, "", r#"{"name":"Notes app"}"#],
    )
    .await;
    let note = r#"{"kind":1,"content":"remember me","tags":[]}"#;
    let profile = r#"{"kind":0,"content":"{}","tags":[]}"#;

    // Approve and remember notes always: the next note signs without asking.
    let id = app.send("sign_event", &[note]).await;
    let ask = desktop.next_ask().await;
    desktop
        .service
        .decide(&ask.key, true, Remember::Always)
        .unwrap();
    assert!(
        app.answer(&id).await["result"]
            .as_str()
            .unwrap()
            .contains("remember me")
    );
    let again = app.call("sign_event", &[note]).await;
    assert!(again["result"].as_str().unwrap().contains("remember me"));
    assert!(desktop.service.pending().is_empty());

    // Reject profiles always: the next profile is refused without asking.
    let id = app.send("sign_event", &[profile]).await;
    let ask = desktop.next_ask().await;
    desktop
        .service
        .decide(&ask.key, false, Remember::Always)
        .unwrap();
    assert_eq!(app.answer(&id).await["error"], "Rejected.");
    assert_eq!(
        app.call("sign_event", &[profile]).await["error"],
        "Rejected."
    );

    // Basic trust approves routine requests; a remembered deny still wins.
    let connection = desktop
        .gooti
        .lock()
        .unwrap()
        .vault()
        .bunker_info()
        .unwrap()
        .connections[0]
        .id
        .clone();
    desktop
        .gooti
        .lock()
        .unwrap()
        .vault_mut()
        .bunker_set_trust(&connection, Trust::Basic)
        .unwrap();
    let reaction = r#"{"kind":7,"content":"+","tags":[]}"#;
    assert!(app.call("sign_event", &[reaction]).await["result"].is_string());
    assert_eq!(
        app.call("sign_event", &[profile]).await["error"],
        "Rejected."
    );

    let detail = desktop
        .gooti
        .lock()
        .unwrap()
        .vault()
        .bunker_connection(&connection)
        .unwrap();
    assert_eq!(detail.trust, Trust::Basic);
    assert_eq!(detail.rules.len(), 2);

    // Everything is in Activity, newest first, with the app's name and the reason.
    desktop
        .gooti
        .lock()
        .unwrap()
        .vault_mut()
        .flush_activity()
        .unwrap();
    let activity =
        serde_json::to_value(desktop.gooti.lock().unwrap().vault().activity().unwrap()).unwrap();
    let rows: Vec<(String, bool, String)> = activity
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "request")
        .map(|e| {
            (
                e["kind"].to_string(),
                e["approved"].as_bool().unwrap(),
                e["reason"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("0".into(), false, "remembered".into()),
            ("7".into(), true, "trustLevel".into()),
            ("0".into(), false, "remembered".into()),
            ("0".into(), false, "user".into()),
            ("1".into(), true, "remembered".into()),
            ("1".into(), true, "user".into()),
        ]
    );
    assert_eq!(
        activity.as_array().unwrap().last().unwrap()["type"],
        "connected"
    );
    assert_eq!(activity[0]["app"], "Notes app");
}

/// Against the real relay: publishes a few kind 24133 messages. Run on request:
/// `cargo test -p gooti-core --test bunker_flow -- --ignored`
/// (`GOOTI_BUNKER_RELAY` picks another relay).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "talks to relay.getgooti.com"]
async fn two_identities_answer_over_relay_getgooti_com() {
    let relay = std::env::var("GOOTI_BUNKER_RELAY")
        .unwrap_or_else(|_| "wss://relay.getgooti.com".to_string());
    let _ = rustls::crypto::ring::default_provider().install_default();
    let desktop = Desktop::start(&relay);
    let bob = desktop
        .gooti
        .lock()
        .unwrap()
        .vault_mut()
        .add_identity("Bob", NewKey::Generate)
        .unwrap();
    let (alice_signer, alice_secret) = desktop.link();
    let (bob_signer, bob_secret) = desktop.link_for(&bob.id);
    // A wss connection takes longer than one on localhost.
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(
        desktop
            .service
            .relay_status()
            .iter()
            .all(|s| s.state == gooti_core::bunker::pool::RelayState::Connected),
        "{:?}",
        desktop.service.relay_status()
    );

    let mut alice_app = App::connect(&relay, &alice_signer).await;
    let mut bob_app = App::connect(&relay, &bob_signer).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(
        alice_app
            .call("connect", &[&alice_signer, &alice_secret])
            .await["result"],
        "ack"
    );
    assert_eq!(
        bob_app.call("connect", &[&bob_signer, &bob_secret]).await["result"],
        "ack"
    );
    assert_eq!(
        alice_app.call("get_public_key", &[]).await["result"],
        desktop.identity_pubkey
    );
    assert_eq!(
        bob_app.call("get_public_key", &[]).await["result"],
        bob.pubkey
    );
    assert_eq!(alice_app.call("logout", &[]).await["result"], "ack");
    assert_eq!(bob_app.call("logout", &[]).await["result"], "ack");
}

/// The sidebar shows how many relays are connected, so the UI hears each change at once.
#[tokio::test(flavor = "multi_thread")]
async fn the_ui_hears_when_a_relay_connects_or_fails() {
    let (relay, _) = start_relay(Mode::Normal).await;
    let mut desktop = Desktop::start(&relay);
    desktop.link();
    desktop
        .relays_until(|status| status.len() == 1 && status[0].state == RelayState::Connected)
        .await;

    // Nothing listens on port 1: the relay fails.
    let dead = "ws://127.0.0.1:1".to_string();
    desktop
        .gooti
        .lock()
        .unwrap()
        .vault_mut()
        .bunker_set_relays(std::slice::from_ref(&dead))
        .unwrap();
    desktop.link();
    // The first link still waits for an app, so its relay stays too.
    desktop
        .relays_until(|status| {
            status.len() == 2
                && status
                    .iter()
                    .any(|s| s.url == dead && matches!(s.state, RelayState::Failed(_)))
        })
        .await;

    // Locking stops the bunker and drops every relay.
    desktop.service.stop();
    desktop.relays_until(<[RelayStatus]>::is_empty).await;
}
