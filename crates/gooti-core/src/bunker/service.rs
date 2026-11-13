//! The running bunker: relays in, vault in the middle, answers out, and the requests that wait
//! for the user. Runs only while the vault is unlocked: the signer keys are sealed in it.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use nostr::event::{Event, EventBuilder, EventId, FinalizeEvent, Kind, Tag};
use serde::Serialize;
use tokio::sync::mpsc;

use super::handler::{Ask, Outcome};
use super::message::{Response, Scheme, response_event};
use super::policy::{Policy, Reason, Remember, rule_for};
use super::pool::{RelayPool, RelayStatus};
use crate::activity::{Activity, ActivityEntry};
use crate::error::Result;
use crate::gooti::Gooti;
use crate::time;

/// An unanswered request is refused after this, so the app is not left hanging.
pub const ASK_TIMEOUT_MS: i64 = 5 * 60 * 1000;
const SEEN_LIMIT: usize = 1000;

/// What the UI hears about.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Notice {
    /// A new request waits for Approve or Reject.
    Asked { ask: Box<Ask> },
    /// The waiting list changed: answered, timed out, or cleared.
    Pending,
    /// An app connected, logged out, or was removed.
    Connections,
    /// Something new in Activity.
    Activity,
    /// A relay connected, was lost, was added, or was dropped. Read `relay_status()`.
    Relays,
}

pub type Notify = Arc<dyn Fn(Notice) + Send + Sync>;

pub struct BunkerService {
    gooti: Arc<Mutex<Gooti>>,
    pool: RelayPool,
    pending: Mutex<Vec<Ask>>,
    seen: Mutex<VecDeque<EventId>>,
    notify: Notify,
}

impl BunkerService {
    /// Starts listening. Call from inside a tokio runtime.
    pub fn start(gooti: Arc<Mutex<Gooti>>, notify: Notify) -> Arc<BunkerService> {
        let (incoming, mut events) = mpsc::unbounded_channel::<Event>();
        let auth_gooti = Arc::clone(&gooti);
        let auth = Arc::new(move |relay: &str, challenge: &str| {
            auth_events(&auth_gooti, relay, challenge)
        });
        let service = Arc::new(BunkerService {
            gooti,
            pool: RelayPool::new(incoming, auth),
            pending: Mutex::new(Vec::new()),
            seen: Mutex::new(VecDeque::new()),
            notify,
        });
        let worker = Arc::downgrade(&service);
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                let Some(service) = worker.upgrade() else {
                    break;
                };
                service.on_event(&event);
            }
        });
        // Ends when the service and its pool are dropped.
        let mut relay_changes = service.pool.changes();
        let relay_notify = Arc::clone(&service.notify);
        tokio::spawn(async move {
            while relay_changes.changed().await.is_ok() {
                relay_notify(Notice::Relays);
            }
        });
        let timer = Arc::downgrade(&service);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                let Some(service) = timer.upgrade() else {
                    break;
                };
                service.expire(time::now_ms());
            }
        });
        service.refresh();
        service
    }

    /// Re-reads which relays and keys to listen on. Call after a change to links,
    /// connections, relays, identities, or the vault's lock.
    pub fn refresh(&self) {
        let listen = self.gooti().vault().bunker_listen();
        self.pool.set(&listen.relays, listen.signer_pubkeys);
    }

    pub fn relay_status(&self) -> Vec<RelayStatus> {
        self.pool.status()
    }

    pub fn pending(&self) -> Vec<Ask> {
        self.pending_lock().clone()
    }

    /// Stops listening and drops the waiting requests (they cannot be answered while locked).
    /// Call before locking the vault, so recorded activity is saved.
    pub fn stop(&self) {
        let _ = self.gooti().vault_mut().flush_activity();
        self.pool.stop();
        let had = !self.pending_lock().is_empty();
        self.pending_lock().clear();
        if had {
            (self.notify)(Notice::Pending);
        }
    }

    /// Connects the app behind a pasted `nostrconnect://` link and tells it so.
    pub fn accept_nostrconnect(&self, link: &str, identity_id: &str) -> Result<()> {
        let reply = self
            .gooti()
            .vault_mut()
            .bunker_accept_nostrconnect(link, identity_id)?;
        self.refresh();
        let response = Response::ok(&uuid::Uuid::new_v4().to_string(), reply.secret);
        if let Some(event) = response_event(
            &reply.signer,
            &reply.client_pubkey,
            &response,
            Scheme::Nip44,
        ) {
            self.pool.publish(&reply.relays, &event);
        }
        (self.notify)(Notice::Connections);
        Ok(())
    }

    /// The user's answer to a waiting request, and how long to remember it.
    /// `false` when it is no longer waiting.
    pub fn decide(&self, key: &str, approve: bool, remember: Remember) -> Result<bool> {
        let ask = {
            let mut pending = self.pending_lock();
            let Some(at) = pending.iter().position(|ask| ask.key == key) else {
                return Ok(false);
            };
            pending.remove(at)
        };
        let policy = if approve { Policy::Allow } else { Policy::Deny };
        if let Some(rule) = rule_for(
            remember,
            &ask.method,
            ask.details.kind,
            policy,
            time::now_ms(),
        ) {
            // The app may have been removed meanwhile; the answer still goes out.
            let _ = self
                .gooti()
                .vault_mut()
                .bunker_remember(&ask.connection_id, rule);
        }
        if approve {
            self.carry_out(&ask, Reason::User)?;
        } else {
            self.refuse(&ask, Reason::User)?;
        }
        (self.notify)(Notice::Pending);
        Ok(true)
    }

    /// Signs, encrypts or decrypts with the identity key, answers, and logs it.
    fn carry_out(&self, ask: &Ask, reason: Reason) -> Result<()> {
        let result = self.gooti().vault().bunker_perform(ask)?;
        let (response, error) = match result {
            Ok(result) => (Response::ok(&ask.request_id, result), None),
            Err(error) => (Response::error(&ask.request_id, error.clone()), Some(error)),
        };
        self.answer(ask, &response)?;
        self.log_request(ask, true, reason, error);
        Ok(())
    }

    fn refuse(&self, ask: &Ask, reason: Reason) -> Result<()> {
        let text = if reason == Reason::TimedOut {
            "Nobody answered on the desktop in time."
        } else {
            "Rejected."
        };
        self.answer(ask, &Response::error(&ask.request_id, text))?;
        self.log_request(ask, false, reason, None);
        Ok(())
    }

    fn log_request(&self, ask: &Ask, approved: bool, reason: Reason, error: Option<String>) {
        let mut gooti = self.gooti();
        let app = gooti.vault().bunker_app_name(&ask.connection_id);
        gooti.vault_mut().record_activity(ActivityEntry {
            at: time::now_ms(),
            what: Activity::Request {
                app,
                connection_id: ask.connection_id.clone(),
                identity_pubkey: ask.identity_pubkey.clone(),
                method: ask.method.clone(),
                kind: ask.details.kind,
                approved,
                reason,
                error,
                site: ask.details.site.clone(),
            },
        });
        drop(gooti);
        (self.notify)(Notice::Activity);
    }

    fn on_event(&self, event: &Event) {
        if !self.first_time(event.id) {
            return;
        }
        let handled = match self
            .gooti()
            .vault_mut()
            .bunker_handle(event, time::now_ms())
        {
            Ok(Some(handled)) => handled,
            // Not ours, or the vault was locked meanwhile.
            Ok(None) | Err(_) => return,
        };
        match handled.outcome {
            Outcome::Reply(response) => {
                if let Some(reply) = response_event(
                    &handled.signer,
                    &handled.client_pubkey,
                    &response,
                    handled.scheme,
                ) {
                    self.pool.publish(&handled.reply_relays, &reply);
                }
            }
            Outcome::Approve(ask, reason) => {
                let _ = self.carry_out(&ask, reason);
            }
            Outcome::Deny(ask) => {
                let _ = self.refuse(&ask, Reason::Remembered);
            }
            Outcome::Ask(ask) => {
                self.pending_lock().push((*ask).clone());
                (self.notify)(Notice::Asked { ask });
            }
            Outcome::Ignore => {}
        }
        if let Some(change) = handled.connection_change {
            let mut gooti = self.gooti();
            gooti.vault_mut().record_activity(ActivityEntry {
                at: time::now_ms(),
                what: change,
            });
            drop(gooti);
            self.refresh();
            (self.notify)(Notice::Connections);
            (self.notify)(Notice::Activity);
        }
    }

    fn expire(&self, now: i64) {
        let expired: Vec<Ask> = {
            let mut pending = self.pending_lock();
            let (old, keep): (Vec<Ask>, Vec<Ask>) = pending
                .drain(..)
                .partition(|ask| now - ask.received_at >= ASK_TIMEOUT_MS);
            *pending = keep;
            old
        };
        if expired.is_empty() {
            return;
        }
        for ask in &expired {
            let _ = self.answer(
                ask,
                &Response::error(&ask.request_id, "Nobody answered on the desktop in time."),
            );
        }
        (self.notify)(Notice::Pending);
    }

    fn answer(&self, ask: &Ask, response: &Response) -> Result<()> {
        let (signer, relays) = {
            let gooti = self.gooti();
            let vault = gooti.vault();
            (
                vault.bunker_signer_for_ask(ask)?,
                vault.bunker_reply_relays(&ask.client_pubkey, &ask.identity_pubkey)?,
            )
        };
        if let Some(event) = signer
            .and_then(|signer| response_event(&signer, &ask.client_pubkey, response, ask.scheme))
        {
            self.pool.publish(&relays, &event);
        }
        Ok(())
    }

    /// Each relay delivers the same request; it is handled once.
    fn first_time(&self, id: EventId) -> bool {
        let mut seen = self.seen.lock().unwrap_or_else(|p| p.into_inner());
        if seen.contains(&id) {
            return false;
        }
        if seen.len() >= SEEN_LIMIT {
            seen.pop_front();
        }
        seen.push_back(id);
        true
    }

    fn gooti(&self) -> MutexGuard<'_, Gooti> {
        self.gooti.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn pending_lock(&self) -> MutexGuard<'_, Vec<Ask>> {
        self.pending.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// NIP-42: one AUTH event per signer key the bunker listens with.
fn auth_events(gooti: &Arc<Mutex<Gooti>>, relay: &str, challenge: &str) -> Vec<Event> {
    let gooti = gooti.lock().unwrap_or_else(|p| p.into_inner());
    let Ok(signers) = gooti.vault().bunker_signer_keys() else {
        return Vec::new();
    };
    signers
        .iter()
        .filter_map(|keys| {
            let tags = [
                Tag::parse(["relay", relay]).ok()?,
                Tag::parse(["challenge", challenge]).ok()?,
            ];
            EventBuilder::new(Kind::Authentication, "")
                .tags(tags)
                .finalize(keys)
                .ok()
        })
        .collect()
}
