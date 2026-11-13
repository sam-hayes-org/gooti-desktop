//! What the bunker does with one request. No network and no locks: the service calls this with
//! the vault's bunker data and sends whatever it returns.

use nostr::event::{EventBuilder, FinalizeEvent, Kind, Tag};
use nostr::key::{Keys, PublicKey};
use nostr::nips::{nip04, nip44};
use nostr::types::Timestamp;
use serde::Serialize;

use super::message::{Incoming, Response, Scheme};
use super::policy::{Reason, Trust, Verdict, evaluate};
use super::state::{BunkerData, ClientMeta, Connection, INVITE_LIFETIME_MS};

/// Methods that sign or read on the identity's behalf. Each one asks the user (M3).
pub const ASKING_METHODS: [&str; 5] = [
    "sign_event",
    "nip04_encrypt",
    "nip04_decrypt",
    "nip44_encrypt",
    "nip44_decrypt",
];

const LAST_USED_STEP_MS: i64 = 60_000;

pub enum Outcome {
    Reply(Response),
    /// Approved by a remembered choice or the trust level: carry it out now.
    Approve(Box<Ask>, Reason),
    /// Refused by a remembered deny, without asking.
    Deny(Box<Ask>),
    /// Waits for the user. The service answers once they decide.
    Ask(Box<Ask>),
    /// Not for this desktop, or not worth an answer.
    Ignore,
}

/// A request waiting for Approve or Reject.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ask {
    /// Unique on this desktop; the app's own id is not.
    pub key: String,
    pub request_id: String,
    pub connection_id: String,
    pub identity_pubkey: String,
    pub client_pubkey: String,
    /// The signer key the request came to; the answer goes out with it.
    #[serde(skip)]
    pub signer_pubkey: String,
    pub method: String,
    pub params: Vec<String>,
    #[serde(skip)]
    pub scheme: Scheme,
    pub received_at: i64,
    pub details: AskDetails,
}

/// What the approval prompt shows.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskDetails {
    pub kind: Option<u16>,
    pub content: Option<String>,
    pub tags: Vec<Vec<String>>,
    pub created_at: Option<u64>,
    /// The other party of an encrypt or decrypt request.
    pub peer: Option<String>,
    /// The site that asked, as the Gooti extension names it. Only from the "Gooti browsers"
    /// connection; other apps cannot set it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
}

/// Handles a request addressed to the signer key of `identity_pubkey`.
/// `has_private_key` is false for identities this desktop cannot sign for.
/// Returns whether `data` changed and must be saved.
pub fn handle(
    data: &mut BunkerData,
    identity_pubkey: &str,
    has_private_key: bool,
    signer_pubkey: &str,
    incoming: &Incoming,
    now: i64,
) -> (Outcome, bool) {
    let request = &incoming.request;
    let client = &incoming.client_pubkey;
    if !has_private_key {
        return (Outcome::Ignore, false);
    }
    if data.paused {
        return (
            reply_error(request, "Gooti is paused on this desktop."),
            false,
        );
    }

    if request.method == "connect" {
        return connect(data, identity_pubkey, signer_pubkey, incoming, now);
    }

    let Some(connection) = data
        .connections
        .iter_mut()
        .find(|c| c.client_pubkey == *client && c.identity_pubkey == identity_pubkey)
    else {
        return (
            reply_error(request, "Not connected. Connect to this bunker first."),
            false,
        );
    };
    // "Last used" is saved at most once a minute, so a busy app does not rewrite the vault.
    let mut changed = connection
        .last_used_at
        .is_none_or(|last| now - last >= LAST_USED_STEP_MS);
    if changed {
        connection.last_used_at = Some(now);
    }
    let connection = connection.clone();

    let outcome = match request.method.as_str() {
        "ping" => Outcome::Reply(Response::ok(&request.id, "pong")),
        "get_public_key" => Outcome::Reply(Response::ok(&request.id, identity_pubkey)),
        "switch_relays" => {
            let relays = reply_relays(data, &connection);
            Outcome::Reply(Response::ok(
                &request.id,
                serde_json::to_string(&relays).expect("a list of strings serializes"),
            ))
        }
        "logout" => {
            data.connections.retain(|c| c.id != connection.id);
            data.removed.push(connection.id.clone());
            changed = true;
            Outcome::Reply(Response::ok(&request.id, "ack"))
        }
        method if ASKING_METHODS.contains(&method) => match details(method, &request.params) {
            Ok(mut details) => {
                if connection.browsers {
                    details.site = site_param(method, &request.params);
                }
                decide(
                    &connection,
                    now,
                    Box::new(Ask {
                        key: uuid::Uuid::new_v4().to_string(),
                        request_id: request.id.clone(),
                        connection_id: connection.id.clone(),
                        identity_pubkey: identity_pubkey.to_string(),
                        client_pubkey: client.clone(),
                        signer_pubkey: signer_pubkey.to_string(),
                        method: method.to_string(),
                        params: request.params.clone(),
                        scheme: incoming.scheme,
                        received_at: now,
                        details,
                    }),
                )
            }
            Err(error) => reply_error(request, error),
        },
        other => reply_error(request, &format!("Unsupported method: {other}")),
    };
    (outcome, changed)
}

/// The trust level and remembered choices of the app decide; otherwise the user does.
fn decide(connection: &Connection, now: i64, ask: Box<Ask>) -> Outcome {
    match evaluate(
        connection.trust,
        &connection.rules,
        &ask.method,
        ask.details.kind,
        now,
    ) {
        (Verdict::Allow, reason) => Outcome::Approve(ask, reason),
        (Verdict::Deny, _) => Outcome::Deny(ask),
        (Verdict::Ask, _) => Outcome::Ask(ask),
    }
}

fn connect(
    data: &mut BunkerData,
    identity_pubkey: &str,
    signer_pubkey: &str,
    incoming: &Incoming,
    now: i64,
) -> (Outcome, bool) {
    let request = &incoming.request;
    let client = &incoming.client_pubkey;
    // Apps send the signer key; some send the identity key. Anything else is not for us.
    let target = request
        .params
        .first()
        .map(String::as_str)
        .unwrap_or(signer_pubkey);
    if target != signer_pubkey && target != identity_pubkey {
        return (Outcome::Ignore, false);
    }
    if data
        .connections
        .iter()
        .any(|c| c.client_pubkey == *client && c.identity_pubkey == identity_pubkey)
    {
        return (Outcome::Reply(Response::ok(&request.id, "ack")), false);
    }
    let secret = request
        .params
        .get(1)
        .map(String::as_str)
        .unwrap_or_default();
    data.invites
        .retain(|invite| now - invite.created_at < INVITE_LIFETIME_MS);
    let Some(at) = data.invites.iter().position(|invite| {
        !secret.is_empty() && invite.secret == secret && invite.identity_pubkey == identity_pubkey
    }) else {
        return (
            reply_error(
                request,
                "This connection link was used before or has expired.",
            ),
            true,
        );
    };
    let invite = data.invites.remove(at);
    let meta = request
        .params
        .get(3)
        .map(|m| ClientMeta::from_json(m))
        .unwrap_or_default();
    data.connections.push(Connection {
        id: uuid::Uuid::new_v4().to_string(),
        identity_pubkey: identity_pubkey.to_string(),
        client_pubkey: client.clone(),
        // The relays the link named. A later change of the default relays leaves this app working.
        relays: if invite.relays.is_empty() {
            data.relays.clone()
        } else {
            invite.relays
        },
        name: meta.name,
        url: meta.url,
        image: meta.image,
        perms: request.params.get(2).filter(|p| !p.is_empty()).cloned(),
        created_at: now,
        last_used_at: Some(now),
        trust: Trust::default(),
        rules: Vec::new(),
        browsers: false,
    });
    (Outcome::Reply(Response::ok(&request.id, "ack")), true)
}

/// Where answers to this app go: its own relays, or the bunker's.
pub fn reply_relays(data: &BunkerData, connection: &Connection) -> Vec<String> {
    app_relays(data, connection).to_vec()
}

/// An app's relays. Empty only for an app stored before 2026-11-12 and not yet given its own
/// (`BunkerData::pin_app_relays`): the default relays.
pub fn app_relays<'a>(data: &'a BunkerData, connection: &'a Connection) -> &'a Vec<String> {
    if connection.relays.is_empty() {
        &data.relays
    } else {
        &connection.relays
    }
}

fn reply_error(request: &super::message::Request, error: &str) -> Outcome {
    Outcome::Reply(Response::error(&request.id, error))
}

fn details(method: &str, params: &[String]) -> Result<AskDetails, &'static str> {
    if method == "sign_event" {
        let template = parse_template(params.first().ok_or("Missing event.")?)?;
        return Ok(AskDetails {
            kind: Some(template.kind),
            content: Some(template.content),
            tags: template.tags,
            created_at: template.created_at,
            peer: None,
            site: None,
        });
    }
    let peer = params.first().ok_or("Missing public key.")?;
    PublicKey::from_hex(peer).map_err(|_| "Invalid public key.")?;
    if params.get(1).is_none() {
        return Err("Missing text.");
    }
    Ok(AskDetails {
        peer: Some(peer.clone()),
        content: if method.ends_with("encrypt") {
            params.get(1).cloned()
        } else {
            None
        },
        ..AskDetails::default()
    })
}

/// The Gooti extension adds `{"site": host}` after the standard parameters (2.1 and later).
fn site_param(method: &str, params: &[String]) -> Option<String> {
    let at = if method == "sign_event" { 1 } else { 2 };
    let value: serde_json::Value = serde_json::from_str(params.get(at)?).ok()?;
    let site = value.get("site")?.as_str()?.trim();
    let plain = !site.is_empty()
        && site.len() <= 253
        && !site.contains('/')
        && !site.chars().any(|c| c.is_whitespace() || c.is_control());
    plain.then(|| site.to_string())
}

struct Template {
    kind: u16,
    content: String,
    tags: Vec<Vec<String>>,
    created_at: Option<u64>,
}

fn parse_template(json: &str) -> Result<Template, &'static str> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|_| "Invalid event.")?;
    let kind = value
        .get("kind")
        .and_then(|k| k.as_u64())
        .and_then(|k| u16::try_from(k).ok())
        .ok_or("Invalid event kind.")?;
    let content = value
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or_default()
        .to_string();
    let tags = match value.get("tags") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(tags) => serde_json::from_value::<Vec<Vec<String>>>(tags.clone())
            .map_err(|_| "Invalid event tags.")?,
    };
    let created_at = value.get("created_at").and_then(|c| c.as_u64());
    Ok(Template {
        kind,
        content,
        tags,
        created_at,
    })
}

/// Carries out an approved request with the identity's key. `Err` is the text sent back.
pub fn perform(keys: &Keys, method: &str, params: &[String]) -> Result<String, String> {
    match method {
        "sign_event" => {
            let template = parse_template(params.first().map(String::as_str).unwrap_or_default())?;
            let tags = template
                .tags
                .iter()
                .map(|tag| Tag::parse(tag.clone()).map_err(|_| "Invalid event tags.".to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            let mut builder =
                EventBuilder::new(Kind::from(template.kind), template.content).tags(tags);
            if let Some(created_at) = template.created_at {
                builder = builder.custom_created_at(Timestamp::from_secs(created_at));
            }
            let event = builder
                .finalize(keys)
                .map_err(|_| "Could not sign.".to_string())?;
            serde_json::to_string(&event).map_err(|_| "Could not sign.".to_string())
        }
        "nip04_encrypt" | "nip04_decrypt" | "nip44_encrypt" | "nip44_decrypt" => {
            let peer = params
                .first()
                .and_then(|p| PublicKey::from_hex(p).ok())
                .ok_or("Invalid public key.")?;
            let text = params.get(1).ok_or("Missing text.")?;
            let secret = keys.secret_key();
            let result = match method {
                "nip04_encrypt" => nip04::encrypt(secret, &peer, text).map_err(|e| e.to_string()),
                "nip04_decrypt" => {
                    nip04::decrypt(secret, &peer, text.as_str()).map_err(|e| e.to_string())
                }
                "nip44_encrypt" => nip44::encrypt(secret, &peer, text, nip44::Version::V2)
                    .map_err(|e| e.to_string()),
                _ => nip44::decrypt(secret, &peer, text).map_err(|e| e.to_string()),
            };
            result.map_err(|_| "Could not encrypt or decrypt.".to_string())
        }
        other => Err(format!("Unsupported method: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use nostr::event::Event;

    use super::*;
    use crate::bunker::message::Request;
    use crate::bunker::state::Invite;

    const IDENTITY: &str = "4f355bdcb7cc0af728ef3cceb9615d90684bb5b2ca5f859ab0f0b704075871aa";
    const SIGNER: &str = "466d7fcae563e5cb09a0d1870bb580344804617879a14949cf22285f1bae3f27";
    const CLIENT: &str = "eff37350d839ce3707332348af4549a96051bd695d3223af4aabce4993531d86";
    const HOUR: i64 = 60 * 60 * 1000;

    fn incoming(method: &str, params: &[&str]) -> Incoming {
        Incoming {
            client_pubkey: CLIENT.into(),
            request: Request {
                id: "r1".into(),
                method: method.into(),
                params: params.iter().map(|p| p.to_string()).collect(),
            },
            scheme: Scheme::Nip44,
        }
    }

    fn with_invite() -> BunkerData {
        BunkerData {
            invites: vec![Invite {
                secret: "s3cret".into(),
                identity_pubkey: IDENTITY.into(),
                created_at: 0,
                relays: vec!["wss://link.example".into()],
            }],
            ..BunkerData::default()
        }
    }

    fn run(data: &mut BunkerData, request: Incoming, now: i64) -> Outcome {
        handle(data, IDENTITY, true, SIGNER, &request, now).0
    }

    fn reply(outcome: Outcome) -> Response {
        match outcome {
            Outcome::Reply(response) => response,
            Outcome::Ask(_) | Outcome::Approve(..) | Outcome::Deny(_) => {
                panic!("not a direct reply")
            }
            Outcome::Ignore => panic!("ignored instead of replying"),
        }
    }

    fn connected() -> BunkerData {
        let mut data = with_invite();
        reply(run(&mut data, incoming("connect", &[SIGNER, "s3cret"]), 10));
        data
    }

    #[test]
    fn connect_takes_a_secret_once_and_only_within_an_hour() {
        let mut data = with_invite();
        let meta = r#"{"name":"Coracle","url":"https://coracle.social"}"#;
        let response = reply(run(
            &mut data,
            incoming("connect", &[SIGNER, "s3cret", "sign_event:1", meta]),
            10,
        ));
        assert_eq!(response.result, "ack");
        assert!(data.invites.is_empty());
        let connection = &data.connections[0];
        assert_eq!(connection.name.as_deref(), Some("Coracle"));
        assert_eq!(connection.perms.as_deref(), Some("sign_event:1"));

        // The same app again is simply acknowledged.
        assert_eq!(
            reply(run(&mut data, incoming("connect", &[SIGNER, "s3cret"]), 20)).result,
            "ack"
        );

        // Another app with the used secret is refused.
        let mut other = incoming("connect", &[SIGNER, "s3cret"]);
        other.client_pubkey = IDENTITY.into();
        assert!(reply(run(&mut data, other, 30)).error.is_some());

        let mut late = with_invite();
        assert!(
            reply(run(
                &mut late,
                incoming("connect", &[SIGNER, "s3cret"]),
                HOUR + 1
            ))
            .error
            .is_some()
        );
        assert!(late.connections.is_empty());
    }

    #[test]
    fn connect_without_a_secret_or_for_another_key_does_not_connect() {
        let mut data = with_invite();
        assert!(
            reply(run(&mut data, incoming("connect", &[SIGNER, ""]), 10))
                .error
                .is_some()
        );
        assert!(matches!(
            run(&mut data, incoming("connect", &[CLIENT, "s3cret"]), 10),
            Outcome::Ignore
        ));
        assert!(data.connections.is_empty());
    }

    #[test]
    fn unknown_apps_get_an_error_and_nothing_else() {
        let mut data = BunkerData::default();
        let response = reply(run(&mut data, incoming("get_public_key", &[]), 10));
        assert_eq!(
            response.error.as_deref(),
            Some("Not connected. Connect to this bunker first.")
        );
    }

    #[test]
    fn only_gooti_browsers_can_name_the_site() {
        let event = r#"{"kind":1,"content":"hi","tags":[],"created_at":5}"#;
        let site = r#"{"site":"primal.net"}"#;
        let peer = "ab".repeat(32);
        let site_of = |outcome: Outcome| match outcome {
            Outcome::Ask(ask) => ask.details.site,
            _ => panic!("expected an ask"),
        };

        let mut data = connected();
        assert_eq!(
            site_of(run(&mut data, incoming("sign_event", &[event, site]), 20)),
            None,
            "an ordinary app cannot set it"
        );

        data.connections[0].browsers = true;
        data.connections[0].trust = crate::bunker::policy::Trust::Ask;
        assert_eq!(
            site_of(run(&mut data, incoming("sign_event", &[event, site]), 20)).as_deref(),
            Some("primal.net")
        );
        assert_eq!(
            site_of(run(
                &mut data,
                incoming("nip44_encrypt", &[&peer, "text", site]),
                20
            ))
            .as_deref(),
            Some("primal.net")
        );
        for odd in [
            r#"{"site":"a b"}"#,
            r#"{"site":"x.com/path"}"#,
            r#"{"site":""}"#,
            "primal.net",
        ] {
            assert_eq!(
                site_of(run(&mut data, incoming("sign_event", &[event, odd]), 20)),
                None,
                "{odd}"
            );
        }
        assert_eq!(
            site_of(run(&mut data, incoming("sign_event", &[event]), 20)),
            None
        );
    }

    #[test]
    fn connected_apps_get_ping_key_and_relays_without_asking() {
        let mut data = connected();
        assert_eq!(
            reply(run(&mut data, incoming("ping", &[]), 20)).result,
            "pong"
        );
        assert_eq!(
            reply(run(&mut data, incoming("get_public_key", &[]), 20)).result,
            IDENTITY
        );
        let relays: Vec<String> =
            serde_json::from_str(&reply(run(&mut data, incoming("switch_relays", &[]), 20)).result)
                .unwrap();
        // The relays of the link it connected with, not the default relays.
        assert_eq!(relays, ["wss://link.example"]);
        // Connected at 10; "last used" moves only after a minute.
        assert_eq!(data.connections[0].last_used_at, Some(10));
        let (_, changed) = handle(
            &mut data,
            IDENTITY,
            true,
            SIGNER,
            &incoming("ping", &[]),
            70_010,
        );
        assert!(changed);
        assert_eq!(data.connections[0].last_used_at, Some(70_010));
    }

    #[test]
    fn signing_and_encryption_ask_first() {
        let mut data = connected();
        let event =
            r#"{"kind":1,"content":"hello","tags":[["t","gooti"]],"created_at":1700000000}"#;
        let Outcome::Ask(ask) = run(&mut data, incoming("sign_event", &[event]), 20) else {
            panic!("expected a question");
        };
        assert_eq!(ask.method, "sign_event");
        assert_eq!(ask.details.kind, Some(1));
        assert_eq!(ask.details.content.as_deref(), Some("hello"));
        assert_eq!(
            ask.details.tags,
            vec![vec!["t".to_string(), "gooti".to_string()]]
        );

        let Outcome::Ask(ask) = run(&mut data, incoming("nip44_decrypt", &[CLIENT, "abc"]), 20)
        else {
            panic!("expected a question");
        };
        assert_eq!(ask.details.peer.as_deref(), Some(CLIENT));
        assert_eq!(ask.details.content, None);
    }

    #[test]
    fn the_trust_level_and_remembered_choices_decide_before_the_user() {
        use crate::bunker::policy::{Policy, Rule};
        let note = r#"{"kind":1,"content":"hi"}"#;
        let profile = r#"{"kind":0,"content":"{}"}"#;

        let mut data = connected();
        data.connections[0].trust = Trust::Basic;
        assert!(matches!(
            run(&mut data, incoming("sign_event", &[note]), 20),
            Outcome::Approve(_, Reason::TrustLevel)
        ));
        assert!(matches!(
            run(&mut data, incoming("sign_event", &[profile]), 20),
            Outcome::Ask(_)
        ));
        assert!(matches!(
            run(&mut data, incoming("nip44_decrypt", &[CLIENT, "x"]), 20),
            Outcome::Approve(..)
        ));

        data.connections[0].trust = Trust::Trust;
        assert!(matches!(
            run(&mut data, incoming("sign_event", &[profile]), 20),
            Outcome::Approve(..)
        ));

        data.connections[0].rules.push(Rule {
            method: "sign_event".into(),
            kind: Some(0),
            policy: Policy::Deny,
            created_at: 0,
            expires_at: None,
        });
        assert!(matches!(
            run(&mut data, incoming("sign_event", &[profile]), 20),
            Outcome::Deny(_)
        ));

        data.connections[0].trust = Trust::Ask;
        data.connections[0].rules.push(Rule {
            method: "sign_event".into(),
            kind: Some(1),
            policy: Policy::Allow,
            created_at: 0,
            expires_at: None,
        });
        assert!(matches!(
            run(&mut data, incoming("sign_event", &[note]), 20),
            Outcome::Approve(_, Reason::Remembered)
        ));
    }

    #[test]
    fn malformed_requests_are_refused_without_asking() {
        let mut data = connected();
        for (method, params) in [
            ("sign_event", vec!["not json"]),
            ("sign_event", vec![r#"{"kind":70000}"#]),
            ("sign_event", vec![r#"{"kind":1,"tags":[[1,2]]}"#]),
            ("nip44_encrypt", vec!["nothex", "x"]),
            ("nip04_decrypt", vec![CLIENT]),
            ("create_account", vec![]),
        ] {
            assert!(
                reply(run(&mut data, incoming(method, &params), 20))
                    .error
                    .is_some(),
                "{method}"
            );
        }
    }

    #[test]
    fn logout_ends_the_session_and_pause_stops_everything() {
        let mut data = connected();
        assert_eq!(
            reply(run(&mut data, incoming("logout", &[]), 20)).result,
            "ack"
        );
        assert!(data.connections.is_empty());

        let mut data = connected();
        data.paused = true;
        assert_eq!(
            reply(run(&mut data, incoming("ping", &[]), 20))
                .error
                .as_deref(),
            Some("Gooti is paused on this desktop.")
        );
    }

    #[test]
    fn identities_without_a_key_here_are_left_alone() {
        let mut data = connected();
        let (outcome, _) = handle(
            &mut data,
            IDENTITY,
            false,
            SIGNER,
            &incoming("ping", &[]),
            20,
        );
        assert!(matches!(outcome, Outcome::Ignore));
    }

    #[test]
    fn approved_requests_sign_and_encrypt_with_the_identity_key() {
        let keys = Keys::generate();
        let template =
            r#"{"kind":1,"content":"hello","tags":[["t","gooti"]],"created_at":1700000000}"#;
        let signed: Event =
            serde_json::from_str(&perform(&keys, "sign_event", &[template.into()]).unwrap())
                .unwrap();
        signed.verify().unwrap();
        assert_eq!(signed.pubkey, keys.public_key());
        assert_eq!(signed.created_at.as_secs(), 1_700_000_000);
        assert_eq!(signed.content, "hello");

        let peer = Keys::generate();
        let peer_hex = peer.public_key().to_hex();
        for (encrypt, decrypt) in [
            ("nip44_encrypt", "nip44_decrypt"),
            ("nip04_encrypt", "nip04_decrypt"),
        ] {
            let ciphertext =
                perform(&keys, encrypt, &[peer_hex.clone(), "secret note".into()]).unwrap();
            // The peer can read it, and so can we.
            let me = keys.public_key().to_hex();
            assert_eq!(
                perform(&peer, decrypt, &[me, ciphertext.clone()]).unwrap(),
                "secret note"
            );
            assert_eq!(
                perform(&keys, decrypt, &[peer_hex.clone(), ciphertext]).unwrap(),
                "secret note"
            );
        }
    }
}
