//! Long-lived relay connections for the bunker. One task per relay: it reconnects with backoff,
//! keeps a kind 24133 subscription for each signer key, answers NIP-42 AUTH, and sends replies.
//!
//! One subscription per key, not one filter with every key: relays such as strfry with
//! `requireAuthorOrTag` (relay.getgooti.com) refuse a filter with more than one `#p` value.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use nostr::event::Event;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

use super::message::NOSTR_CONNECT_KIND;
use crate::backup::relay::MAX_WAIT;

/// Subscription ids are this prefix and the start of the signer key.
const SUBSCRIPTION: &str = "gooti-bunker-";
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// Replies waiting for a relay that is down. Older ones are dropped; the app gave up on them.
const MAX_QUEUED: usize = 50;
/// Requests a little older than now still count: a relay may deliver them late.
const SINCE_SLACK_SECS: u64 = 30;
/// When the pool drops a relay, it waits this long for the relay to confirm the replies it just
/// sent. strfry drops events from a connection that closes before they are stored, so a logout
/// answer, sent just before the bunker stops listening, would otherwise be lost.
const LINGER: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state", content = "reason")]
pub enum RelayState {
    Connecting,
    Connected,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelayStatus {
    pub url: String,
    #[serde(flatten)]
    pub state: RelayState,
}

/// Signs NIP-42 AUTH events for a relay's challenge, one per signer key.
pub type AuthSigner = Arc<dyn Fn(&str, &str) -> Vec<Event> + Send + Sync>;

enum Command {
    Publish(Event),
    Stop,
}

struct RelayTask {
    commands: mpsc::UnboundedSender<Command>,
    state: Arc<Mutex<RelayState>>,
}

#[derive(Clone)]
pub struct RelayPool {
    inner: Arc<Inner>,
}

struct Inner {
    relays: Mutex<HashMap<String, RelayTask>>,
    incoming: mpsc::UnboundedSender<Event>,
    pubkeys: watch::Sender<Vec<String>>,
    auth: AuthSigner,
    /// Counts changes to `status()`: a relay added, dropped, connected, or lost.
    changes: Arc<watch::Sender<u64>>,
}

impl RelayPool {
    /// Events for the subscribed keys arrive on `incoming`, possibly once per relay.
    pub fn new(incoming: mpsc::UnboundedSender<Event>, auth: AuthSigner) -> RelayPool {
        let (pubkeys, _) = watch::channel(Vec::new());
        let (changes, _) = watch::channel(0);
        RelayPool {
            inner: Arc::new(Inner {
                relays: Mutex::new(HashMap::new()),
                incoming,
                pubkeys,
                auth,
                changes: Arc::new(changes),
            }),
        }
    }

    /// Changes each time `status()` would answer differently. Closes when the pool is dropped.
    pub fn changes(&self) -> watch::Receiver<u64> {
        self.inner.changes.subscribe()
    }

    /// Connects to `relays` (and drops the others) and listens for these signer keys.
    /// Must be called from inside a tokio runtime.
    pub fn set(&self, relays: &[String], signer_pubkeys: Vec<String>) {
        self.inner.pubkeys.send_replace(signer_pubkeys);
        let mut tasks = self.inner.relays.lock().unwrap_or_else(|p| p.into_inner());
        let before = tasks.len();
        tasks.retain(|url, task| {
            let keep = relays.contains(url);
            if !keep {
                let _ = task.commands.send(Command::Stop);
            }
            keep
        });
        if tasks.len() != before {
            bump(&self.inner.changes);
        }
        for url in relays {
            if tasks.contains_key(url) {
                continue;
            }
            let (commands, receiver) = mpsc::unbounded_channel();
            let state = Arc::new(Mutex::new(RelayState::Connecting));
            tokio::spawn(run_relay(
                url.clone(),
                receiver,
                Arc::clone(&state),
                self.inner.incoming.clone(),
                self.inner.pubkeys.subscribe(),
                Arc::clone(&self.inner.auth),
                Arc::clone(&self.inner.changes),
            ));
            tasks.insert(url.clone(), RelayTask { commands, state });
            bump(&self.inner.changes);
        }
    }

    /// Sends to these relays. A relay that is down sends it once it is back.
    pub fn publish(&self, relays: &[String], event: &Event) {
        let tasks = self.inner.relays.lock().unwrap_or_else(|p| p.into_inner());
        for url in relays {
            if let Some(task) = tasks.get(url) {
                let _ = task.commands.send(Command::Publish(event.clone()));
            }
        }
    }

    pub fn status(&self) -> Vec<RelayStatus> {
        let tasks = self.inner.relays.lock().unwrap_or_else(|p| p.into_inner());
        let mut status: Vec<RelayStatus> = tasks
            .iter()
            .map(|(url, task)| RelayStatus {
                url: url.clone(),
                state: task.state.lock().unwrap_or_else(|p| p.into_inner()).clone(),
            })
            .collect();
        status.sort_by(|a, b| a.url.cmp(&b.url));
        status
    }

    pub fn stop(&self) {
        self.set(&[], Vec::new());
    }
}

fn bump(changes: &watch::Sender<u64>) {
    changes.send_modify(|count| *count = count.wrapping_add(1));
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn run_relay(
    url: String,
    mut commands: mpsc::UnboundedReceiver<Command>,
    state: Arc<Mutex<RelayState>>,
    incoming: mpsc::UnboundedSender<Event>,
    mut pubkeys: watch::Receiver<Vec<String>>,
    auth: AuthSigner,
    changes: Arc<watch::Sender<u64>>,
) {
    let set_state = |next: RelayState| {
        let mut current = state.lock().unwrap_or_else(|p| p.into_inner());
        if *current != next {
            *current = next;
            bump(&changes);
        }
    };
    let mut queue: VecDeque<Event> = VecDeque::new();
    let mut backoff = Duration::from_secs(1);
    loop {
        set_state(RelayState::Connecting);
        let failure = match crate::backup::relay::connect_socket(&url, MAX_WAIT).await {
            Err(reason) => reason,
            Ok(mut socket) => {
                set_state(RelayState::Connected);
                backoff = Duration::from_secs(1);
                match serve(
                    &url,
                    &mut socket,
                    &mut commands,
                    &mut queue,
                    &incoming,
                    &mut pubkeys,
                    &auth,
                )
                .await
                {
                    Served::Stopped => {
                        let _ = socket.close(None).await;
                        return;
                    }
                    Served::Lost(reason) => reason,
                }
            }
        };
        set_state(RelayState::Failed(failure));
        // Wait before trying again, but stop at once when asked and keep replies for later.
        let wait = tokio::time::sleep(backoff);
        tokio::pin!(wait);
        loop {
            tokio::select! {
                _ = &mut wait => break,
                command = commands.recv() => match command {
                    Some(Command::Publish(event)) => enqueue(&mut queue, event),
                    Some(Command::Stop) | None => return,
                },
            }
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

enum Served {
    Stopped,
    Lost(String),
}

async fn serve(
    url: &str,
    socket: &mut Socket,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    queue: &mut VecDeque<Event>,
    incoming: &mpsc::UnboundedSender<Event>,
    pubkeys: &mut watch::Receiver<Vec<String>>,
    auth: &AuthSigner,
) -> Served {
    let mut subscribed = HashSet::new();
    // Replies sent that the relay has not confirmed with OK yet.
    let mut unconfirmed: HashSet<String> = HashSet::new();
    let keys = pubkeys.borrow_and_update().clone();
    if let Err(reason) = subscribe(socket, &mut subscribed, &keys, false).await {
        return Served::Lost(reason);
    }
    while let Some(event) = queue.pop_front() {
        if let Err(reason) = send(socket, json!(["EVENT", event])).await {
            queue.push_front(event);
            return Served::Lost(reason);
        }
        unconfirmed.insert(event.id.to_hex());
    }
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(Command::Publish(event)) => {
                    if let Err(reason) = send(socket, json!(["EVENT", event])).await {
                        enqueue(queue, event);
                        return Served::Lost(reason);
                    }
                    // A relay that never confirms must not make this grow.
                    if unconfirmed.len() >= MAX_QUEUED {
                        unconfirmed.clear();
                    }
                    unconfirmed.insert(event.id.to_hex());
                }
                Some(Command::Stop) | None => {
                    linger(socket, &mut unconfirmed).await;
                    return Served::Stopped;
                }
            },
            changed = pubkeys.changed() => {
                if changed.is_err() {
                    return Served::Stopped;
                }
                let keys = pubkeys.borrow_and_update().clone();
                if let Err(reason) = subscribe(socket, &mut subscribed, &keys, false).await {
                    return Served::Lost(reason);
                }
            }
            message = socket.next() => {
                let text = match message {
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Close(_))) | None => return Served::Lost("The relay closed the connection.".into()),
                    Some(Ok(_)) => continue,
                    Some(Err(error)) => return Served::Lost(error.to_string()),
                };
                let Ok(frame) = serde_json::from_str::<Vec<Value>>(text.as_str()) else { continue };
                match frame.first().and_then(Value::as_str) {
                    Some("EVENT") if frame.get(1).and_then(Value::as_str).is_some_and(|sub| sub.starts_with(SUBSCRIPTION)) => {
                        if let Some(event) = frame.get(2).and_then(|e| serde_json::from_value::<Event>(e.clone()).ok()) {
                            let _ = incoming.send(event);
                        }
                    }
                    Some("OK") => {
                        if let Some(id) = frame.get(1).and_then(Value::as_str) {
                            unconfirmed.remove(id);
                        }
                    }
                    Some("AUTH") => {
                        let Some(challenge) = frame.get(1).and_then(Value::as_str) else { continue };
                        for event in auth(url, challenge) {
                            if let Err(reason) = send(socket, json!(["AUTH", event])).await {
                                return Served::Lost(reason);
                            }
                        }
                        // A relay that wanted AUTH may have closed the subscriptions. Ask again.
                        let keys = pubkeys.borrow().clone();
                        if let Err(reason) = subscribe(socket, &mut subscribed, &keys, true).await {
                            return Served::Lost(reason);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Waits until the relay confirmed every reply in `unconfirmed`, at most `LINGER`.
async fn linger(socket: &mut Socket, unconfirmed: &mut HashSet<String>) {
    let wait = async {
        while !unconfirmed.is_empty() {
            let Some(Ok(message)) = socket.next().await else {
                return;
            };
            let Message::Text(text) = message else {
                continue;
            };
            let Ok(frame) = serde_json::from_str::<Vec<Value>>(text.as_str()) else {
                continue;
            };
            if frame.first().and_then(Value::as_str) == Some("OK")
                && let Some(id) = frame.get(1).and_then(Value::as_str)
            {
                unconfirmed.remove(id);
            }
        }
    };
    let _ = tokio::time::timeout(LINGER, wait).await;
}

fn subscription_id(pubkey: &str) -> String {
    format!("{SUBSCRIPTION}{}", &pubkey[..pubkey.len().min(16)])
}

/// Brings the relay's subscriptions in line with `pubkeys`: closes the ones for keys that are
/// gone and opens the new ones. `again` opens every one, for a relay that closed them.
async fn subscribe(
    socket: &mut Socket,
    subscribed: &mut HashSet<String>,
    pubkeys: &[String],
    again: bool,
) -> Result<(), String> {
    let gone: Vec<String> = subscribed
        .iter()
        .filter(|key| !pubkeys.contains(key))
        .cloned()
        .collect();
    for key in gone {
        send(socket, json!(["CLOSE", subscription_id(&key)])).await?;
        subscribed.remove(&key);
    }
    let since = (crate::time::now_ms() / 1000) as u64 - SINCE_SLACK_SECS;
    for key in pubkeys {
        if !again && subscribed.contains(key) {
            continue;
        }
        let filter = json!({ "kinds": [NOSTR_CONNECT_KIND], "#p": [key], "since": since });
        send(socket, json!(["REQ", subscription_id(key), filter])).await?;
        subscribed.insert(key.clone());
    }
    Ok(())
}

async fn send(socket: &mut Socket, value: Value) -> Result<(), String> {
    socket
        .send(Message::text(value.to_string()))
        .await
        .map_err(|error| error.to_string())
}

fn enqueue(queue: &mut VecDeque<Event>, event: Event) {
    if queue.len() >= MAX_QUEUED {
        queue.pop_front();
    }
    queue.push_back(event);
}
