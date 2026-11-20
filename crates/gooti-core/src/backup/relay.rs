//! Talking to relays over WebSocket. Port of `backup/pool.ts`.
//!
//! One failed relay does not fail the call when another relay accepts the event or returns a
//! blob. Every relay failing is an error that lists each relay's own reason.

use std::future::Future;
use std::time::Duration;

use futures_util::future::join_all;
use futures_util::{SinkExt, StreamExt};
use nostr::event::Event;
use serde_json::{Value, json};
use tokio::time::{Instant, timeout, timeout_at};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use super::event::BACKUP_KIND;
use crate::error::{Error, RelayFailure, Result};

/// Same limit as the extension for connecting, for EOSE and for an OK.
pub const MAX_WAIT: Duration = Duration::from_secs(8);

pub trait RelayIo: Send + Sync {
    /// The relay URLs that accepted the event.
    fn publish(
        &self,
        relays: &[String],
        event: &Event,
    ) -> impl Future<Output = Result<Vec<String>>> + Send;
    /// The newest event of `pubkey` with this `d` tag, or `None` when a relay answered and had none.
    fn fetch_newest(
        &self,
        relays: &[String],
        pubkey: &str,
        d_tag: &str,
    ) -> impl Future<Output = Result<Option<Event>>> + Send;
    /// `fetch_newest`, and in the same request every event with `also_d`. The second is `None`
    /// when this way to reach relays cannot do that; the caller then does without.
    fn fetch_newest_also(
        &self,
        relays: &[String],
        pubkey: &str,
        d_tag: &str,
        also_d: &str,
    ) -> impl Future<Output = Result<(Option<Event>, Option<Vec<Event>>)>> + Send {
        let _ = also_d;
        async move { Ok((self.fetch_newest(relays, pubkey, d_tag).await?, None)) }
    }
    /// Every kind 30078 event of `pubkey` (device records, and the shared record, which callers
    /// skip). A relay that cannot be reached is left out; all of them failing is an error.
    fn fetch_all(
        &self,
        relays: &[String],
        pubkey: &str,
    ) -> impl Future<Output = Result<Vec<Event>>> + Send;
}

/// Opens a fresh connection per call and closes it after, like the extension.
#[derive(Clone, Default)]
pub struct WebSocketRelays {
    pub wait: Option<Duration>,
}

impl WebSocketRelays {
    fn wait(&self) -> Duration {
        self.wait.unwrap_or(MAX_WAIT)
    }

    /// The newest event with `d_tag`, and every event with `also_d`, in one request per relay.
    async fn newest_and(
        &self,
        relays: &[String],
        pubkey: &str,
        d_tag: &str,
        also_d: Option<&str>,
    ) -> Result<(Option<Event>, Vec<Event>)> {
        let wait = self.wait();
        let tags: Vec<&str> = std::iter::once(d_tag).chain(also_d).collect();
        let filter = json!({ "kinds": [BACKUP_KIND], "authors": [pubkey], "#d": tags });
        let results = join_all(relays.iter().map(|url| fetch_one(url, &filter, wait))).await;
        let mut reached = Vec::new();
        let mut failures = Vec::new();
        for (url, result) in relays.iter().zip(results) {
            match result {
                Ok(events) => reached.push(events),
                Err(reason) => failures.push(RelayFailure {
                    url: url.clone(),
                    reason,
                }),
            }
        }
        if reached.is_empty() {
            return Err(Error::Relays {
                message: "No relay could be reached.".into(),
                failures,
            });
        }
        let (main, also): (Vec<Event>, Vec<Event>) = reached
            .into_iter()
            .flatten()
            .partition(|event| event.tags.identifier().as_deref() == Some(d_tag));
        let also = match also_d {
            Some(also_d) => also
                .into_iter()
                .filter(|event| event.tags.identifier().as_deref() == Some(also_d))
                .collect(),
            None => Vec::new(),
        };
        let newest = main.into_iter().reduce(|newest, event| {
            if event.created_at > newest.created_at {
                event
            } else {
                newest
            }
        });
        // A relay that did not answer may hold the backup. Only when every relay answered is there
        // none, and only then may the caller publish this desktop's vault as the first.
        if newest.is_none() && !failures.is_empty() {
            return Err(Error::Relays {
                message: "A relay could not be reached, and the others have no backup.".into(),
                failures,
            });
        }
        Ok((newest, also))
    }
}

impl RelayIo for WebSocketRelays {
    async fn publish(&self, relays: &[String], event: &Event) -> Result<Vec<String>> {
        let wait = self.wait();
        let results = join_all(relays.iter().map(|url| publish_one(url, event, wait))).await;
        let mut accepted = Vec::new();
        let mut failures = Vec::new();
        for (url, result) in relays.iter().zip(results) {
            match result {
                Ok(()) => accepted.push(url.clone()),
                Err(reason) => failures.push(RelayFailure {
                    url: url.clone(),
                    reason,
                }),
            }
        }
        if accepted.is_empty() {
            return Err(Error::Relays {
                message: "No relay accepted your backup.".into(),
                failures,
            });
        }
        Ok(accepted)
    }

    async fn fetch_newest(
        &self,
        relays: &[String],
        pubkey: &str,
        d_tag: &str,
    ) -> Result<Option<Event>> {
        Ok(self.newest_and(relays, pubkey, d_tag, None).await?.0)
    }

    async fn fetch_newest_also(
        &self,
        relays: &[String],
        pubkey: &str,
        d_tag: &str,
        also_d: &str,
    ) -> Result<(Option<Event>, Option<Vec<Event>>)> {
        let (newest, also) = self.newest_and(relays, pubkey, d_tag, Some(also_d)).await?;
        Ok((newest, Some(also)))
    }

    async fn fetch_all(&self, relays: &[String], pubkey: &str) -> Result<Vec<Event>> {
        let wait = self.wait();
        // Relays cannot filter a d tag by prefix.
        let filter = json!({ "kinds": [BACKUP_KIND], "authors": [pubkey] });
        let results = join_all(relays.iter().map(|url| fetch_one(url, &filter, wait))).await;
        let mut events = Vec::new();
        let mut failures = Vec::new();
        let mut reached = false;
        for (url, result) in relays.iter().zip(results) {
            match result {
                Ok(found) => {
                    reached = true;
                    events.extend(found);
                }
                Err(reason) => failures.push(RelayFailure {
                    url: url.clone(),
                    reason,
                }),
            }
        }
        if !reached {
            return Err(Error::Relays {
                message: "No relay could be reached.".into(),
                failures,
            });
        }
        Ok(events)
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// rustls finds more than one crypto backend in this app's dependencies and will not guess.
fn install_tls_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // An error means another part of the app installed one first, which is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Opens a WebSocket to a relay, or says why it could not.
pub(crate) async fn connect_socket(
    url: &str,
    wait: Duration,
) -> std::result::Result<Socket, String> {
    connect(url, wait).await
}

async fn connect(url: &str, wait: Duration) -> std::result::Result<Socket, String> {
    install_tls_provider();
    match timeout(wait, connect_async(url)).await {
        Ok(Ok((socket, _))) => Ok(socket),
        Ok(Err(error)) => Err(reason_text(&error.to_string())),
        Err(_) => Err("Connection timed out.".into()),
    }
}

/// Collects events until EOSE. A relay that never sends EOSE ends the wait with what it sent.
async fn fetch_one(
    url: &str,
    filter: &Value,
    wait: Duration,
) -> std::result::Result<Vec<Event>, String> {
    let mut socket = connect(url, wait).await?;
    let sub = "gooti-backup";
    send(&mut socket, json!(["REQ", sub, filter])).await?;
    let deadline = Instant::now() + wait;
    let mut events = Vec::new();
    let outcome = loop {
        let message = match timeout_at(deadline, socket.next()).await {
            Err(_) => break Ok(()),
            Ok(None) => break Err("The relay closed the connection.".to_string()),
            Ok(Some(Err(error))) => break Err(reason_text(&error.to_string())),
            Ok(Some(Ok(message))) => message,
        };
        let Some(frame) = frame(&message) else {
            continue;
        };
        match frame.first().and_then(Value::as_str) {
            Some("EVENT") if frame.get(1) == Some(&json!(sub)) => {
                if let Some(event) = frame
                    .get(2)
                    .and_then(|e| serde_json::from_value::<Event>(e.clone()).ok())
                {
                    events.push(event);
                }
            }
            Some("EOSE") if frame.get(1) == Some(&json!(sub)) => break Ok(()),
            Some("CLOSED") if frame.get(1) == Some(&json!(sub)) => {
                let reason = frame.get(2).and_then(Value::as_str).unwrap_or_default();
                break Err(reason_text(if reason.is_empty() {
                    "The relay closed the connection."
                } else {
                    reason
                }));
            }
            _ => {}
        }
    };
    let _ = send(&mut socket, json!(["CLOSE", sub])).await;
    let _ = socket.close(None).await;
    outcome.map(|()| events)
}

async fn publish_one(url: &str, event: &Event, wait: Duration) -> std::result::Result<(), String> {
    let mut socket = connect(url, wait).await?;
    send(&mut socket, json!(["EVENT", event])).await?;
    let id = event.id.to_hex();
    let deadline = Instant::now() + wait;
    let outcome = loop {
        let message = match timeout_at(deadline, socket.next()).await {
            Err(_) => break Err("publish timed out".to_string()),
            Ok(None) => break Err("The relay closed the connection.".to_string()),
            Ok(Some(Err(error))) => break Err(reason_text(&error.to_string())),
            Ok(Some(Ok(message))) => message,
        };
        let Some(frame) = frame(&message) else {
            continue;
        };
        if frame.first().and_then(Value::as_str) == Some("OK") && frame.get(1) == Some(&json!(id)) {
            let accepted = frame.get(2).and_then(Value::as_bool).unwrap_or(false);
            let reason = frame.get(3).and_then(Value::as_str).unwrap_or_default();
            break if accepted {
                Ok(())
            } else {
                Err(reason_text(if reason.is_empty() {
                    "The relay rejected the backup."
                } else {
                    reason
                }))
            };
        }
    };
    let _ = socket.close(None).await;
    outcome
}

async fn send(socket: &mut Socket, value: Value) -> std::result::Result<(), String> {
    socket
        .send(Message::text(value.to_string()))
        .await
        .map_err(|error| reason_text(&error.to_string()))
}

fn frame(message: &Message) -> Option<Vec<Value>> {
    match message {
        Message::Text(text) => serde_json::from_str::<Vec<Value>>(text.as_str()).ok(),
        _ => None,
    }
}

fn reason_text(reason: &str) -> String {
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        "No reason.".into()
    } else {
        trimmed.into()
    }
}
