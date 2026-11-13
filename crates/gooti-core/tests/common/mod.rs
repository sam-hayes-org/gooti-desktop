//! Test relays: a real WebSocket relay on localhost, and an in-memory one.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use gooti_core::backup::relay::RelayIo;
use gooti_core::{Error, RelayFailure, Result};
use nostr::event::Event;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

/// How the WebSocket test relay behaves.
#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Normal,
    /// Answers every EVENT with `OK false`.
    Reject,
    /// Never sends EOSE.
    Silent,
    /// Sends an AUTH challenge and delivers nothing to a subscription until a valid AUTH.
    Auth,
    /// Like strfry with `filterValidation.requireAuthorOrTag` (relay.getgooti.com): refuses a
    /// subscription unless each filter has exactly one author, `#p` value, or `#e` value.
    /// Also like strfry, it stores an event a moment later, and not at all when the connection
    /// that sent it closes first.
    Strict,
}

/// strfry's `requireAuthorOrTag` and `maxFiltersPerReq = 3`.
fn strict_ok(filters: &[Value]) -> bool {
    let one = |filter: &Value, key: &str| {
        filter
            .get(key)
            .and_then(Value::as_array)
            .is_some_and(|values| values.len() == 1)
    };
    (1..=3).contains(&filters.len())
        && filters
            .iter()
            .all(|f| one(f, "authors") || one(f, "#p") || one(f, "#e"))
}

/// Does `event` match a NIP-01 filter (kinds, authors, #p, since)?
fn matches(filter: &Value, event: &Event) -> bool {
    let list = |key: &str| filter.get(key).and_then(Value::as_array);
    if let Some(kinds) = list("kinds")
        && !kinds
            .iter()
            .any(|k| k.as_u64() == Some(u64::from(event.kind.as_u16())))
    {
        return false;
    }
    if let Some(authors) = list("authors")
        && !authors
            .iter()
            .any(|a| a.as_str() == Some(&event.pubkey.to_hex()))
    {
        return false;
    }
    if let Some(ps) = list("#p") {
        let tagged: Vec<String> = event.tags.public_keys().map(|k| k.to_hex()).collect();
        if !ps
            .iter()
            .any(|p| p.as_str().is_some_and(|p| tagged.iter().any(|t| t == p)))
        {
            return false;
        }
    }
    if let Some(since) = filter.get("since").and_then(Value::as_u64)
        && event.created_at.as_secs() < since
    {
        return false;
    }
    true
}

/// Starts a relay on a free port. Returns its `ws://` URL and the events it stored.
/// New events also go to every open subscription they match, like a real relay.
pub async fn start_relay(mode: Mode) -> (String, Arc<Mutex<Vec<Event>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let stored: Arc<Mutex<Vec<Event>>> = Arc::default();
    let events = Arc::clone(&stored);
    let (live, _) = tokio::sync::broadcast::channel::<Event>(256);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let events = Arc::clone(&events);
            let live_tx = live.clone();
            let mut live_rx = live.subscribe();
            tokio::spawn(async move {
                let Ok(mut socket) = accept_async(stream).await else {
                    return;
                };
                let challenge = "challenge-123";
                let mut authed: Vec<String> = Vec::new();
                if mode == Mode::Auth {
                    let _ = socket
                        .send(Message::text(json!(["AUTH", challenge]).to_string()))
                        .await;
                }
                let mut subs: Vec<(Value, Value)> = Vec::new();
                let mut unstored: Vec<Event> = Vec::new();
                loop {
                    let reply: Vec<Value> = tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_millis(100)), if !unstored.is_empty() => {
                            unstored.drain(..).map(|event| {
                                events.lock().unwrap().push(event.clone());
                                let _ = live_tx.send(event.clone());
                                json!(["OK", event.id.to_hex(), true, ""])
                            }).collect()
                        }
                        event = live_rx.recv() => {
                            let Ok(event) = event else { continue };
                            subs.iter()
                                .filter(|(_, filter)| matches(filter, &event))
                                .filter(|_| mode != Mode::Auth || !authed.is_empty())
                                .map(|(sub, _)| json!(["EVENT", sub, event]))
                                .collect()
                        }
                        message = socket.next() => {
                            let Some(Ok(Message::Text(text))) = message else { return };
                            let frame: Vec<Value> = serde_json::from_str(text.as_str()).unwrap();
                            match frame[0].as_str().unwrap() {
                                "EVENT" => {
                                    let event: Event = serde_json::from_value(frame[1].clone()).unwrap();
                                    let id = event.id.to_hex();
                                    if mode == Mode::Reject {
                                        vec![json!(["OK", id, false, "blocked: only vault events"])]
                                    } else if mode == Mode::Strict {
                                        unstored.push(event);
                                        vec![]
                                    } else {
                                        events.lock().unwrap().push(event.clone());
                                        let _ = live_tx.send(event);
                                        vec![json!(["OK", id, true, ""])]
                                    }
                                }
                                "AUTH" => {
                                    let event: Event = serde_json::from_value(frame[1].clone()).unwrap();
                                    let ok = event.verify().is_ok()
                                        && event.kind.as_u16() == 22242
                                        && event.tags.iter().any(|t| t.as_slice() == ["challenge", challenge]);
                                    if ok {
                                        authed.push(event.pubkey.to_hex());
                                    }
                                    vec![json!(["OK", event.id.to_hex(), ok, ""])]
                                }
                                "REQ" => {
                                    let sub = frame[1].clone();
                                    let filter = frame[2].clone();
                                    subs.retain(|(s, _)| *s != sub);
                                    if mode == Mode::Strict && !strict_ok(&frame[2..]) {
                                        vec![json!(["CLOSED", sub, "error: filter must have exactly one author, p tag, or e tag"])]
                                    } else {
                                    subs.push((sub.clone(), filter.clone()));
                                    if mode == Mode::Auth && authed.is_empty() {
                                        vec![json!(["CLOSED", sub, "auth-required: sign in first"])]
                                    } else {
                                        let mut out: Vec<Value> = events
                                            .lock()
                                            .unwrap()
                                            .iter()
                                            .filter(|e| matches(&filter, e))
                                            .map(|e| json!(["EVENT", sub, e]))
                                            .collect();
                                        if mode != Mode::Silent {
                                            out.push(json!(["EOSE", sub]));
                                        }
                                        out
                                    }
                                    }
                                }
                                "CLOSE" => {
                                    subs.retain(|(s, _)| *s != frame[1]);
                                    vec![]
                                }
                                _ => vec![],
                            }
                        }
                    };
                    for message in reply {
                        if socket
                            .send(Message::text(message.to_string()))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            });
        }
    });
    (url, stored)
}

/// Relays in memory, keyed by URL. A URL in `down` fails like an unreachable relay.
#[derive(Clone, Default)]
pub struct MemoryRelays {
    pub events: Arc<Mutex<Vec<(String, Event)>>>,
    pub down: Arc<Mutex<Vec<String>>>,
}

impl MemoryRelays {
    pub fn put(&self, relay: &str, event: Event) {
        self.events.lock().unwrap().push((relay.to_string(), event));
    }

    pub fn count(&self) -> usize {
        self.events.lock().unwrap().len()
    }
}

impl RelayIo for MemoryRelays {
    async fn publish(&self, relays: &[String], event: &Event) -> Result<Vec<String>> {
        let down = self.down.lock().unwrap().clone();
        let up: Vec<String> = relays
            .iter()
            .filter(|r| !down.contains(r))
            .cloned()
            .collect();
        if up.is_empty() {
            return Err(Error::Relays {
                message: "No relay accepted your backup.".into(),
                failures: relays
                    .iter()
                    .map(|url| RelayFailure {
                        url: url.clone(),
                        reason: "Connection refused".into(),
                    })
                    .collect(),
            });
        }
        for relay in &up {
            self.put(relay, event.clone());
        }
        Ok(up)
    }

    async fn fetch_newest(
        &self,
        relays: &[String],
        pubkey: &str,
        d_tag: &str,
    ) -> Result<Option<Event>> {
        let down = self.down.lock().unwrap().clone();
        if relays.iter().all(|r| down.contains(r)) {
            return Err(Error::Relays {
                message: "No relay could be reached.".into(),
                failures: vec![],
            });
        }
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|(relay, event)| {
                relays.contains(relay)
                    && !down.contains(relay)
                    && event.pubkey.to_hex() == pubkey
                    && event.tags.identifier().as_deref() == Some(d_tag)
            })
            .map(|(_, event)| event.clone())
            .max_by_key(|event| event.created_at))
    }
}
