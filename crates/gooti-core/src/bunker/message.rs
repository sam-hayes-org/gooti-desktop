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

/// Why a request failed, for programs: the `code` next to `error` in a reply. NIP-46 has only the
/// error text, which is written for people and may change; the code does not. Apps that do not
/// know it ignore it. Never rename a value: servers such as a Gooti MCP server act on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The client key has no connection to this identity: never connected, disconnected on the
    /// desktop, or logged out. Connect again with a new link.
    NotConnected,
    /// The connection link was used before or has expired.
    LinkInvalid,
    /// Gooti is paused on this desktop.
    Paused,
    /// Someone pressed Reject, or a remembered choice rejects it.
    Rejected,
    /// Nobody answered the question on the desktop in time.
    TimedOut,
    /// The method is not one this bunker knows.
    UnsupportedMethod,
    /// A parameter is missing or invalid.
    InvalidParams,
    /// Approved, but signing, encrypting or decrypting failed.
    Failed,
    /// A browser key that was denied or disconnected. The extension makes a new key.
    BrowserDisconnected,
    /// A browser `connect` without the secret from the backup.
    BrowserNotKnown,
    /// A browser still on Gooti 2.0's retired shared key.
    BrowserUpdate,
    /// An AI agent asked for something its scope does not cover.
    NotInScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Response {
    pub id: String,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<ErrorCode>,
}

impl Response {
    pub fn ok(id: &str, result: impl Into<String>) -> Response {
        Response {
            id: id.into(),
            result: result.into(),
            error: None,
            code: None,
        }
    }

    pub fn error(id: &str, code: ErrorCode, error: impl Into<String>) -> Response {
        Response {
            id: id.into(),
            result: String::new(),
            error: Some(error.into()),
            code: Some(code),
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

/// A reply from a bunker, as an app reads it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Reply {
    pub id: String,
    /// Some bunkers send a number or an object; it is kept as text.
    #[serde(default, deserialize_with = "text")]
    pub result: String,
    /// `None` for success. Some bunkers send an empty error on success; that counts as none.
    #[serde(default, deserialize_with = "optional_text")]
    pub error: Option<String>,
    /// Only Gooti Desktop sends it (versions after 0.2.0). A code this version does not know is `None`.
    #[serde(default, deserialize_with = "known_code")]
    pub code: Option<ErrorCode>,
}

fn text<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::String(text) => text,
        Value::Null => String::new(),
        other => other.to_string(),
    })
}

fn optional_text<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Ok(Some(text(deserializer)?).filter(|text| !text.is_empty()))
}

fn known_code<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ErrorCode>, D::Error> {
    Ok(serde_json::from_value(Value::deserialize(deserializer)?).ok())
}

/// What an app sends a bunker: a NIP-44 request event from `client` to `signer_pubkey`.
/// `None` when `signer_pubkey` is not a public key.
pub fn app_request<P: Serialize>(
    client: &Keys,
    signer_pubkey: &str,
    id: &str,
    method: &str,
    params: &[P],
) -> Option<Event> {
    build_request(client, signer_pubkey, id, method, params, Scheme::Nip44)
}

/// What an app reads from a bunker's reply event. `None` when it is not a reply for `client`.
pub fn open_reply(event: &Event, client: &Keys) -> Option<Reply> {
    if event.kind != Kind::Custom(NOSTR_CONNECT_KIND) || event.verify().is_err() {
        return None;
    }
    let (plaintext, _) = decrypt_any(client, &event.pubkey, &event.content)?;
    serde_json::from_str(&plaintext).ok()
}

fn build_request<P: Serialize>(
    client: &Keys,
    signer_pubkey: &str,
    id: &str,
    method: &str,
    params: &[P],
    scheme: Scheme,
) -> Option<Event> {
    let signer = PublicKey::from_hex(signer_pubkey).ok()?;
    let plaintext = serde_json::json!({ "id": id, "method": method, "params": params }).to_string();
    let content = match scheme {
        Scheme::Nip44 => {
            nip44::encrypt(client.secret_key(), &signer, plaintext, nip44::Version::V2).ok()?
        }
        Scheme::Nip04 => nip04::encrypt(client.secret_key(), &signer, plaintext).ok()?,
    };
    EventBuilder::new(Kind::Custom(NOSTR_CONNECT_KIND), content)
        .tag(Tag::public_key(signer))
        .finalize(client)
        .ok()
}

/// For tests and the end-to-end checks: a request as an app would send it, in either scheme.
pub fn request_event(
    client: &Keys,
    signer_pubkey: &str,
    id: &str,
    method: &str,
    params: &[&str],
    scheme: Scheme,
) -> Event {
    build_request(client, signer_pubkey, id, method, params, scheme).expect("a hex pubkey")
}

/// For tests: what an app reads from a reply, untyped.
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
    fn errors_carry_an_empty_result_and_a_code() {
        let json = serde_json::to_value(Response::error(
            "7",
            ErrorCode::NotConnected,
            "Not connected.",
        ))
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "id": "7", "result": "", "error": "Not connected.", "code": "not_connected" })
        );
        let ok = serde_json::to_value(Response::ok("8", "pong")).unwrap();
        assert_eq!(ok, serde_json::json!({ "id": "8", "result": "pong" }));
    }

    #[test]
    fn an_app_sends_a_request_and_reads_the_typed_reply() {
        let signer = Keys::generate();
        let client = Keys::generate();
        let params = vec!["{\"kind\":1}".to_string()];
        let event = app_request(
            &client,
            &signer.public_key().to_hex(),
            "9",
            "sign_event",
            &params,
        )
        .unwrap();
        let incoming = open_request(&event, &signer).unwrap();
        assert_eq!(incoming.scheme, Scheme::Nip44);
        assert_eq!(incoming.request.params, params);

        let refused = Response::error("9", ErrorCode::Rejected, "Rejected.");
        let reply = response_event(
            &signer,
            &client.public_key().to_hex(),
            &refused,
            Scheme::Nip44,
        )
        .unwrap();
        let opened = open_reply(&reply, &client).unwrap();
        assert_eq!(opened.id, "9");
        assert_eq!(opened.error.as_deref(), Some("Rejected."));
        assert_eq!(opened.code, Some(ErrorCode::Rejected));

        let stranger = Keys::generate();
        assert!(open_reply(&reply, &stranger).is_none(), "not for this key");
        assert!(app_request(&client, "not a key", "1", "ping", &[] as &[String]).is_none());
    }

    #[test]
    fn replies_from_other_bunkers_are_read_leniently() {
        let read = |json: &str| -> Reply { serde_json::from_str(json).unwrap() };
        let plain = read(r#"{"id":"1","result":"ack"}"#);
        assert_eq!(
            (plain.result.as_str(), plain.error, plain.code),
            ("ack", None, None)
        );
        let empty_error = read(r#"{"id":"1","result":"pong","error":""}"#);
        assert_eq!(empty_error.error, None);
        let number = read(r#"{"id":"1","result":5,"error":null}"#);
        assert_eq!(number.result, "5");
        let unknown = read(r#"{"id":"1","result":"","error":"No.","code":"something_new"}"#);
        assert_eq!(
            (unknown.error.as_deref(), unknown.code),
            (Some("No."), None)
        );
    }

    #[test]
    fn error_codes_keep_their_names() {
        let names = [
            (ErrorCode::NotConnected, "not_connected"),
            (ErrorCode::LinkInvalid, "link_invalid"),
            (ErrorCode::Paused, "paused"),
            (ErrorCode::Rejected, "rejected"),
            (ErrorCode::TimedOut, "timed_out"),
            (ErrorCode::UnsupportedMethod, "unsupported_method"),
            (ErrorCode::InvalidParams, "invalid_params"),
            (ErrorCode::Failed, "failed"),
            (ErrorCode::BrowserDisconnected, "browser_disconnected"),
            (ErrorCode::BrowserNotKnown, "browser_not_known"),
            (ErrorCode::BrowserUpdate, "browser_update"),
            (ErrorCode::NotInScope, "not_in_scope"),
        ];
        for (code, name) in names {
            assert_eq!(serde_json::to_value(code).unwrap(), name);
            assert_eq!(
                serde_json::from_value::<ErrorCode>(name.into()).unwrap(),
                code
            );
        }
    }
}
