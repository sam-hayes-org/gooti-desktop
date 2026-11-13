//! NIP-46 requests and responses on kind 24133 events.
//!
//! Requests are NIP-44 encrypted. Some older apps still use NIP-04; a reply uses the same
//! scheme as the request, so those apps can read it.

use nostr::event::{Event, EventBuilder, FinalizeEvent, Kind, Tag};
use nostr::key::{Keys, PublicKey};
use nostr::nips::{nip04, nip44};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const NOSTR_CONNECT_KIND: u16 = 24133;

/// A request from an app, decrypted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Request {
    pub id: String,
    pub method: String,
    #[serde(default, deserialize_with = "string_params")]
    pub params: Vec<String>,
}

/// Params are strings in NIP-46, but some apps send numbers or objects. Keep them as text.
fn string_params<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    let values = Vec::<Value>::deserialize(deserializer)?;
    Ok(values
        .into_iter()
        .map(|value| match value {
            Value::String(text) => text,
            other => other.to_string(),
        })
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Response {
    pub id: String,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(id: &str, result: impl Into<String>) -> Response {
        Response {
            id: id.into(),
            result: result.into(),
            error: None,
        }
    }

    pub fn error(id: &str, error: impl Into<String>) -> Response {
        Response {
            id: id.into(),
            result: String::new(),
            error: Some(error.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Nip44,
    Nip04,
}

/// An incoming request event, opened with the signer key it is addressed to.
#[derive(Debug, Clone)]
pub struct Incoming {
    pub client_pubkey: String,
    pub request: Request,
    pub scheme: Scheme,
}

/// Decrypts a request event. `None` when it is not a readable request for this key.
pub fn open_request(event: &Event, signer: &Keys) -> Option<Incoming> {
    if event.kind != Kind::Custom(NOSTR_CONNECT_KIND) || event.verify().is_err() {
        return None;
    }
    let (plaintext, scheme) = decrypt_any(signer, &event.pubkey, &event.content)?;
    let request: Request = serde_json::from_str(&plaintext).ok()?;
    Some(Incoming {
        client_pubkey: event.pubkey.to_hex(),
        request,
        scheme,
    })
}

/// The reply event: encrypted for the app, tagged with its pubkey, signed by the signer key.
pub fn response_event(
    signer: &Keys,
    client_pubkey: &str,
    response: &Response,
    scheme: Scheme,
) -> Option<Event> {
    let client = PublicKey::from_hex(client_pubkey).ok()?;
    let plaintext = serde_json::to_string(response).ok()?;
    let content = match scheme {
        Scheme::Nip44 => {
            nip44::encrypt(signer.secret_key(), &client, plaintext, nip44::Version::V2).ok()?
        }
        Scheme::Nip04 => nip04::encrypt(signer.secret_key(), &client, plaintext).ok()?,
    };
    EventBuilder::new(Kind::Custom(NOSTR_CONNECT_KIND), content)
        .tag(Tag::public_key(client))
        .finalize(signer)
        .ok()
}

/// NIP-04 payloads end in `?iv=…`; everything else is tried as NIP-44.
fn decrypt_any(signer: &Keys, sender: &PublicKey, content: &str) -> Option<(String, Scheme)> {
    if content.contains("?iv=") {
        return nip04::decrypt(signer.secret_key(), sender, content)
            .ok()
            .map(|text| (text, Scheme::Nip04));
    }
    nip44::decrypt(signer.secret_key(), sender, content)
        .ok()
        .map(|text| (text, Scheme::Nip44))
}

/// For tests and the end-to-end checks: a request as an app would send it.
pub fn request_event(
    client: &Keys,
    signer_pubkey: &str,
    id: &str,
    method: &str,
    params: &[&str],
    scheme: Scheme,
) -> Event {
    let signer = PublicKey::from_hex(signer_pubkey).expect("a hex pubkey");
    let plaintext = serde_json::json!({ "id": id, "method": method, "params": params }).to_string();
    let content = match scheme {
        Scheme::Nip44 => {
            nip44::encrypt(client.secret_key(), &signer, plaintext, nip44::Version::V2).unwrap()
        }
        Scheme::Nip04 => nip04::encrypt(client.secret_key(), &signer, plaintext).unwrap(),
    };
    EventBuilder::new(Kind::Custom(NOSTR_CONNECT_KIND), content)
        .tag(Tag::public_key(signer))
        .finalize(client)
        .unwrap()
}

/// For tests: what an app reads from a reply.
pub fn open_response(event: &Event, client: &Keys) -> Option<Value> {
    let (plaintext, _) = decrypt_any(client, &event.pubkey, &event.content)?;
    serde_json::from_str(&plaintext).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_round_trips_in_both_schemes_and_the_reply_uses_the_same() {
        let signer = Keys::generate();
        let client = Keys::generate();
        for scheme in [Scheme::Nip44, Scheme::Nip04] {
            let event = request_event(
                &client,
                &signer.public_key().to_hex(),
                "1",
                "ping",
                &[],
                scheme,
            );
            let incoming = open_request(&event, &signer).unwrap();
            assert_eq!(incoming.scheme, scheme);
            assert_eq!(incoming.client_pubkey, client.public_key().to_hex());
            assert_eq!(incoming.request.method, "ping");

            let reply = response_event(
                &signer,
                &incoming.client_pubkey,
                &Response::ok("1", "pong"),
                scheme,
            )
            .unwrap();
            assert_eq!(
                reply.tags.public_keys().next().unwrap().to_hex(),
                client.public_key().to_hex()
            );
            let opened = open_response(&reply, &client).unwrap();
            assert_eq!(opened, serde_json::json!({ "id": "1", "result": "pong" }));
        }
    }

    #[test]
    fn requests_for_another_key_or_with_bad_json_are_ignored() {
        let signer = Keys::generate();
        let other = Keys::generate();
        let client = Keys::generate();
        let event = request_event(
            &client,
            &other.public_key().to_hex(),
            "1",
            "ping",
            &[],
            Scheme::Nip44,
        );
        assert!(open_request(&event, &signer).is_none());
    }

    #[test]
    fn non_string_params_are_kept_as_text() {
        let request: Request =
            serde_json::from_str(r#"{"id":"1","method":"sign_event","params":[{"kind":1},5]}"#)
                .unwrap();
        assert_eq!(request.params, [r#"{"kind":1}"#, "5"]);
    }

    #[test]
    fn errors_carry_an_empty_result() {
        let json = serde_json::to_value(Response::error("7", "Rejected")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "id": "7", "result": "", "error": "Rejected" })
        );
    }
}
