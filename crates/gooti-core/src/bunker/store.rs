//! The bunker's part of the vault.

use nostr::key::{Keys, SecretKey};
use serde::Serialize;

use super::handler::{self, Ask, Outcome};
use super::message::Incoming;
use super::policy::{Rule, Trust, remember};
use super::state::{
    BrowserDevice, Connection, INVITE_LIFETIME_MS, Invite, SignerKey, browser_code, browser_secret,
};
use super::uri::{bunker_uri, parse_nostrconnect};
use crate::activity::{Activity, ActivityEntry};
use crate::backup::document::{SharedBunker, is_wss_url};
use crate::backup::phrase::hex;
use crate::crypto::random_bytes;
use crate::error::{Error, Result};
use crate::identity::{BrowserLink, Custody, StoredIdentity};
use crate::time;
use crate::vault::{Vault, normalize_relays};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BunkerInfo {
    /// The default relays for apps.
    pub relays: Vec<String>,
    pub paused: bool,
    pub connections: Vec<ConnectionInfo>,
    /// The relays of bunker links still waiting for an app.
    pub link_relays: Vec<String>,
}

/// "Browsers" in Connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserList {
    pub browsers: Vec<BrowserInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserInfo {
    pub client_pubkey: String,
    pub name: String,
    pub code: String,
    pub approved_at: Option<i64>,
    pub last_used_at: Option<i64>,
}

/// "Browsers" on a Desktop only identity's page: how far its browsers may go without asking, and
/// the relays they reach it on. The same for every browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityBrowsers {
    pub trust: Trust,
    pub relays: Vec<String>,
    /// Choices remembered in the approval window, per browser, that still apply.
    pub rules: Vec<BrowserRule>,
}

/// A choice remembered for one browser and this identity. Forget it with the connection's id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserRule {
    pub connection_id: String,
    pub browser: String,
    #[serde(flatten)]
    pub rule: Rule,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub id: String,
    pub identity_id: String,
    pub identity_nick: String,
    pub client_pubkey: String,
    pub name: Option<String>,
    pub url: Option<String>,
    pub image: Option<String>,
    pub perms: Option<String>,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
    pub trust: Trust,
    /// The browsers reaching a Desktop only identity. Cannot be removed here.
    pub browsers: bool,
    /// Where this app reaches the desktop.
    pub relays: Vec<String>,
    /// Another Gooti Desktop answers this app (it answers for the identity), not this one.
    pub served_elsewhere: bool,
}

/// One app with its trust level and remembered choices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDetail {
    #[serde(flatten)]
    pub info: ConnectionInfo,
    pub trust: Trust,
    pub rules: Vec<Rule>,
}

/// What the relay pool should do: which relays to hold and which keys to listen for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listen {
    pub relays: Vec<String>,
    pub signer_pubkeys: Vec<String>,
}

/// The `connect` answer to send for an accepted `nostrconnect://` link.
pub struct ConnectReply {
    pub signer: Keys,
    pub client_pubkey: String,
    pub relays: Vec<String>,
    pub secret: String,
}

/// A request the vault handled: what to send, with which key, to which relays.
pub struct Handled {
    pub outcome: Outcome,
    pub signer: Keys,
    pub reply_relays: Vec<String>,
    pub client_pubkey: String,
    /// The app's encryption; the reply uses the same.
    pub scheme: super::message::Scheme,
    /// An app connected or logged out: what to put in Activity.
    pub connection_change: Option<Activity>,
}

impl Vault {
    pub fn bunker_info(&self) -> Result<BunkerInfo> {
        let unlocked = self.unlocked()?;
        let data = &unlocked.data;
        let connections = data
            .bunker
            .connections
            .iter()
            .filter_map(|connection| {
                let identity = data
                    .identities
                    .iter()
                    .find(|i| i.pubkey == connection.identity_pubkey)?;
                Some(ConnectionInfo {
                    id: connection.id.clone(),
                    identity_id: identity.id.clone(),
                    identity_nick: identity.nick.clone(),
                    client_pubkey: connection.client_pubkey.clone(),
                    name: connection.name.clone(),
                    url: connection.url.clone(),
                    image: connection.image.clone(),
                    perms: connection.perms.clone(),
                    created_at: connection.created_at,
                    last_used_at: connection.last_used_at,
                    trust: connection.trust,
                    browsers: connection.browsers,
                    relays: handler::reply_relays(&data.bunker, connection),
                    served_elsewhere: Vault::served_elsewhere(unlocked, &identity.pubkey),
                })
            })
            .collect();
        let now = time::now_ms();
        let mut link_relays: Vec<String> = Vec::new();
        for invite in &data.bunker.invites {
            if now - invite.created_at < INVITE_LIFETIME_MS {
                for relay in invite_relays(&data.bunker, invite) {
                    if !link_relays.contains(relay) {
                        link_relays.push(relay.clone());
                    }
                }
            }
        }
        Ok(BunkerInfo {
            relays: data.bunker.relays.clone(),
            paused: data.bunker.paused,
            connections,
            link_relays,
        })
    }

    /// A new single-use `bunker://` link for this identity, valid for an hour, on the default relays.
    pub fn bunker_link(&mut self, identity_id: &str) -> Result<String> {
        self.bunker_link_on(identity_id, None)
    }

    /// A new single-use `bunker://` link on these relays (`None`: the default relays). The app
    /// that uses it keeps them.
    pub fn bunker_link_on(
        &mut self,
        identity_id: &str,
        relays: Option<&[String]>,
    ) -> Result<String> {
        let relays = match relays {
            Some(relays) => normalize_bunker_relays(relays)?,
            None => self.unlocked()?.data.bunker.relays.clone(),
        };
        let identity = self.signable(identity_id)?;
        let now = time::now_ms();
        let signer = self.signer_for(&identity)?;
        let unlocked = self.unlocked_mut()?;
        let bunker = &mut unlocked.data.bunker;
        bunker
            .invites
            .retain(|invite| now - invite.created_at < INVITE_LIFETIME_MS);
        let secret = hex(random_bytes::<16>().as_ref());
        bunker.invites.push(Invite {
            secret: secret.clone(),
            identity_pubkey: identity,
            created_at: now,
            relays: relays.clone(),
        });
        let uri = bunker_uri(&signer.public_key().to_hex(), &relays, &secret);
        self.save()?;
        Ok(uri)
    }

    /// Connects the app behind a pasted `nostrconnect://` link. Pasting it is the consent.
    pub fn bunker_accept_nostrconnect(
        &mut self,
        link: &str,
        identity_id: &str,
    ) -> Result<ConnectReply> {
        let link = parse_nostrconnect(link)?;
        let identity = self.signable(identity_id)?;
        let signer = self.signer_for(&identity)?;
        let now = time::now_ms();
        let bunker = &mut self.unlocked_mut()?.data.bunker;
        bunker
            .connections
            .retain(|c| !(c.client_pubkey == link.client_pubkey && c.identity_pubkey == identity));
        bunker.connections.push(Connection {
            id: uuid::Uuid::new_v4().to_string(),
            identity_pubkey: identity.clone(),
            client_pubkey: link.client_pubkey.clone(),
            relays: link.relays.clone(),
            name: link.name,
            url: link.url,
            image: link.image,
            perms: link.perms,
            created_at: now,
            last_used_at: Some(now),
            trust: Trust::default(),
            rules: Vec::new(),
            browsers: false,
        });
        let added = bunker.connections.last().expect("just added");
        let (app, connection_id) = (app_name(added), added.id.clone());
        self.record_activity(ActivityEntry {
            at: now,
            what: Activity::Connected {
                app,
                connection_id,
                identity_pubkey: identity,
            },
        });
        self.save()?;
        Ok(ConnectReply {
            signer,
            client_pubkey: link.client_pubkey,
            relays: link.relays,
            secret: link.secret,
        })
    }

    pub fn bunker_revoke(&mut self, connection_id: &str) -> Result<()> {
        if self.connection(connection_id)?.browsers {
            return Err(Error::BrowsersConnection);
        }
        let bunker = &mut self.unlocked_mut()?.data.bunker;
        let at = bunker
            .connections
            .iter()
            .position(|c| c.id == connection_id)
            .ok_or(Error::ConnectionNotFound)?;
        let removed = bunker.connections.remove(at);
        bunker.removed.push(removed.id.clone());
        self.record_activity(ActivityEntry {
            at: time::now_ms(),
            what: Activity::Disconnected {
                app: app_name(&removed),
                connection_id: removed.id.clone(),
                identity_pubkey: removed.identity_pubkey.clone(),
            },
        });
        self.save()
    }

    /// One app's settings: its trust level and remembered choices.
    pub fn bunker_connection(&self, connection_id: &str) -> Result<ConnectionDetail> {
        let info = self
            .bunker_info()?
            .connections
            .into_iter()
            .find(|c| c.id == connection_id)
            .ok_or(Error::ConnectionNotFound)?;
        let connection = self.connection(connection_id)?;
        let now = time::now_ms();
        Ok(ConnectionDetail {
            info,
            trust: connection.trust,
            rules: connection
                .rules
                .iter()
                .filter(|r| r.expires_at.is_none_or(|e| now < e))
                .cloned()
                .collect(),
        })
    }

    pub fn bunker_set_trust(&mut self, connection_id: &str, trust: Trust) -> Result<()> {
        self.connection_mut(connection_id)?.trust = trust;
        self.save()
    }

    /// Keeps a choice the user made in the approval prompt.
    pub fn bunker_remember(&mut self, connection_id: &str, rule: Rule) -> Result<()> {
        let now = time::now_ms();
        remember(&mut self.connection_mut(connection_id)?.rules, rule, now);
        self.save()
    }

    /// Removes a remembered choice, by method and kind.
    pub fn bunker_forget(
        &mut self,
        connection_id: &str,
        method: &str,
        kind: Option<u16>,
    ) -> Result<()> {
        self.connection_mut(connection_id)?
            .rules
            .retain(|r| !(r.method == method && r.kind == kind));
        self.save()
    }

    /// "Browsers" in Connections: each browser with its own key, and the shared key of Gooti 2.0
    /// browsers while it still works. Blocked browsers are left out.
    pub fn bunker_browsers(&self) -> Result<BrowserList> {
        let bunker = &self.unlocked()?.data.bunker;
        let last_used = |client: &str| {
            bunker
                .connections
                .iter()
                .filter(|c| c.client_pubkey == client)
                .filter_map(|c| c.last_used_at)
                .max()
        };
        let browsers = bunker
            .browsers
            .iter()
            .filter(|b| !b.blocked && b.approved_at.is_some())
            .map(|b| BrowserInfo {
                client_pubkey: b.client_pubkey.clone(),
                name: b.name.clone(),
                code: browser_code(&b.client_pubkey),
                approved_at: b.approved_at,
                last_used_at: last_used(&b.client_pubkey),
            })
            .collect();
        Ok(BrowserList { browsers })
    }

    /// Allow on a browser's request: it signs for this identity now, and for every other
    /// Desktop only identity as soon as it asks.
    pub fn bunker_allow_browser(&mut self, ask: &Ask) -> Result<()> {
        let name = ask
            .details
            .browser
            .as_ref()
            .map(|b| b.name.clone())
            .unwrap_or_else(|| "A browser".into());
        let now = time::now_ms();
        let bunker = &mut self.unlocked_mut()?.data.bunker;
        bunker
            .browsers
            .retain(|b| b.client_pubkey != ask.client_pubkey);
        let browser = BrowserDevice {
            client_pubkey: ask.client_pubkey.clone(),
            name,
            approved_at: Some(now),
            blocked: false,
            blocked_at: None,
        };
        handler::add_browser_connection(bunker, &ask.identity_pubkey, &browser, now);
        bunker.browsers.push(browser.clone());
        self.record_activity(ActivityEntry {
            at: now,
            what: Activity::Connected {
                app: browser.name,
                connection_id: self
                    .unlocked()?
                    .data
                    .bunker
                    .connections
                    .iter()
                    .find(|c| {
                        c.client_pubkey == ask.client_pubkey
                            && c.identity_pubkey == ask.identity_pubkey
                    })
                    .map(|c| c.id.clone())
                    .unwrap_or_default(),
                identity_pubkey: ask.identity_pubkey.clone(),
            },
        });
        self.save_change()
    }

    /// Deny on a browser's request: it cannot ask again with that key for 90 days.
    pub fn bunker_block_browser(&mut self, client_pubkey: &str, name: &str) -> Result<()> {
        let bunker = &mut self.unlocked_mut()?.data.bunker;
        match bunker
            .browsers
            .iter_mut()
            .find(|b| b.client_pubkey == client_pubkey)
        {
            Some(browser) => {
                browser.blocked = true;
                browser.blocked_at = Some(time::now_ms());
            }
            None => bunker.browsers.push(BrowserDevice {
                client_pubkey: client_pubkey.to_string(),
                name: name.to_string(),
                approved_at: None,
                blocked: true,
                blocked_at: Some(time::now_ms()),
            }),
        }
        self.save_change()
    }

    /// "Disconnect": the browser's key stops working for every identity, at once. It stays blocked
    /// for 90 days (`BLOCKED_KEPT_MS`); by then a lost browser is long gone from your hands.
    pub fn bunker_disconnect_browser(&mut self, client_pubkey: &str) -> Result<()> {
        let now = time::now_ms();
        let bunker = &mut self.unlocked_mut()?.data.bunker;
        let browser = bunker
            .browsers
            .iter_mut()
            .find(|b| b.client_pubkey == client_pubkey && !b.blocked)
            .ok_or(Error::ConnectionNotFound)?;
        browser.blocked = true;
        browser.blocked_at = Some(now);
        let name = browser.name.clone();
        let ended: Vec<Connection> = bunker
            .connections
            .iter()
            .filter(|c| c.client_pubkey == client_pubkey)
            .cloned()
            .collect();
        bunker
            .connections
            .retain(|c| c.client_pubkey != client_pubkey);
        bunker.removed.extend(ended.iter().map(|c| c.id.clone()));
        for connection in ended {
            self.record_activity(ActivityEntry {
                at: now,
                what: Activity::Disconnected {
                    app: name.clone(),
                    connection_id: connection.id,
                    identity_pubkey: connection.identity_pubkey,
                },
            });
        }
        self.save_change()
    }

    /// The name Activity and the prompts show for an app.
    pub fn bunker_app_name(&self, connection_id: &str) -> String {
        self.connection(connection_id)
            .map(app_name)
            .unwrap_or_else(|_| "An app".into())
    }

    fn connection(&self, connection_id: &str) -> Result<&Connection> {
        self.unlocked()?
            .data
            .bunker
            .connections
            .iter()
            .find(|c| c.id == connection_id)
            .ok_or(Error::ConnectionNotFound)
    }

    fn connection_mut(&mut self, connection_id: &str) -> Result<&mut Connection> {
        self.unlocked_mut()?
            .data
            .bunker
            .connections
            .iter_mut()
            .find(|c| c.id == connection_id)
            .ok_or(Error::ConnectionNotFound)
    }

    pub fn bunker_set_paused(&mut self, paused: bool) -> Result<()> {
        self.unlocked_mut()?.data.bunker.paused = paused;
        self.save()
    }

    /// Sets the default relays for apps. What is in use keeps its own relays.
    pub fn bunker_set_relays(&mut self, relays: &[String]) -> Result<Vec<String>> {
        let relays = normalize_bunker_relays(relays)?;
        let bunker = &mut self.unlocked_mut()?.data.bunker;
        bunker.relays = relays.clone();
        bunker.relays_at = Some(time::now_ms() as f64);
        self.save()?;
        Ok(relays)
    }

    /// The relays for a Desktop only identity's browsers: these, or the default relays.
    /// Browsers need a `wss://` relay (the extension accepts only those).
    pub(crate) fn browser_relays_or_default(
        &self,
        relays: Option<&[String]>,
    ) -> Result<Vec<String>> {
        let relays = match relays {
            Some(relays) => normalize_bunker_relays(relays)?,
            None => self.unlocked()?.data.bunker.relays.clone(),
        };
        if !relays.iter().any(|r| is_wss_url(r)) {
            return Err(Error::NoBrowserRelay);
        }
        Ok(relays)
    }

    /// "Browsers" on a Desktop only identity's page.
    pub fn bunker_identity_browsers(&self, identity_id: &str) -> Result<IdentityBrowsers> {
        let identity = self.find(identity_id)?;
        if identity.custody != Custody::Desktop {
            return Err(Error::NotDesktopOnly);
        }
        let bunker = &self.unlocked()?.data.bunker;
        let now = time::now_ms();
        let mut rules: Vec<BrowserRule> = bunker
            .connections
            .iter()
            .filter(|c| c.browsers && c.identity_pubkey == identity.pubkey)
            .flat_map(|c| {
                c.rules
                    .iter()
                    .filter(move |r| r.expires_at.is_none_or(|e| now < e))
                    .map(move |r| BrowserRule {
                        connection_id: c.id.clone(),
                        browser: app_name(c),
                        rule: r.clone(),
                    })
            })
            .collect();
        rules.sort_by(|a, b| {
            (&a.browser, &a.rule.method, a.rule.kind).cmp(&(
                &b.browser,
                &b.rule.method,
                b.rule.kind,
            ))
        });
        Ok(IdentityBrowsers {
            trust: bunker.browser_trust_of(&identity.pubkey),
            relays: handler::browser_relays(bunker, &identity.pubkey),
            rules,
        })
    }

    /// How far every browser may go for this Desktop only identity without asking.
    pub fn bunker_set_identity_browser_trust(
        &mut self,
        identity_id: &str,
        trust: Trust,
    ) -> Result<IdentityBrowsers> {
        self.bunker_identity_browsers(identity_id)?;
        let pubkey = self.find(identity_id)?.pubkey.clone();
        let bunker = &mut self.unlocked_mut()?.data.bunker;
        bunker.browser_trust.insert(pubkey.clone(), trust);
        for connection in bunker
            .connections
            .iter_mut()
            .filter(|c| c.browsers && c.identity_pubkey == pubkey)
        {
            connection.trust = trust;
        }
        self.save_change()?;
        self.bunker_identity_browsers(identity_id)
    }

    /// The relays every browser reaches this Desktop only identity on. Backup takes the change to
    /// the browsers.
    pub fn bunker_set_identity_browser_relays(
        &mut self,
        identity_id: &str,
        relays: &[String],
    ) -> Result<IdentityBrowsers> {
        self.bunker_identity_browsers(identity_id)?;
        let relays = self.browser_relays_or_default(Some(relays))?;
        let pubkey = self.find(identity_id)?.pubkey.clone();
        self.set_browser_relays(&pubkey, relays)?;
        self.bunker_identity_browsers(identity_id)
    }

    fn set_browser_relays(&mut self, identity_pubkey: &str, relays: Vec<String>) -> Result<()> {
        let bunker = &mut self.unlocked_mut()?.data.bunker;
        bunker
            .browser_relays
            .insert(identity_pubkey.to_string(), relays.clone());
        for connection in bunker
            .connections
            .iter_mut()
            .filter(|c| c.browsers && c.identity_pubkey == identity_pubkey)
        {
            connection.relays = relays.clone();
        }
        self.save()?;
        // The pointers browsers use name these relays.
        self.restamp_desktop_identities()?;
        Ok(())
    }

    /// Changes the relays browsers use to reach a Desktop only identity, from one of its
    /// browsers' connections. Backup takes the change to the browsers. Other apps chose their
    /// relays when they connected.
    pub fn bunker_set_app_relays(
        &mut self,
        connection_id: &str,
        relays: &[String],
    ) -> Result<Vec<String>> {
        if !self.connection(connection_id)?.browsers {
            return Err(Error::AppRelaysFixed);
        }
        let relays = self.browser_relays_or_default(Some(relays))?;
        let identity = self.connection(connection_id)?.identity_pubkey.clone();
        self.set_browser_relays(&identity, relays.clone())?;
        Ok(relays)
    }

    /// Makes the signer key, and connects the approved browsers on these relays.
    pub(crate) fn ensure_browser_link(
        &mut self,
        identity_id: &str,
        relays: Vec<String>,
    ) -> Result<()> {
        let pubkey = self.find(identity_id)?.pubkey.clone();
        let signer = self.signer_for(&pubkey)?;
        if self.find(identity_id)?.browsers.is_none() {
            let client = Keys::generate();
            self.identity_mut(identity_id)?.browsers = Some(BrowserLink {
                client_secret_hex: client.secret_key().to_secret_hex(),
                ask_site: None,
                signer_pubkey: Some(signer.public_key().to_hex()),
            });
            // Moved to the desktop (again): browsers start at Basic. Set, not left out, so an
            // earlier choice in the desktop record does not come back.
            self.unlocked_mut()?
                .data
                .bunker
                .browser_trust
                .insert(pubkey.clone(), Trust::Basic);
        }
        // Browsers with their own key connect themselves. Approved ones need nothing more.
        let bunker = &mut self.unlocked_mut()?.data.bunker;
        bunker.browser_relays.insert(pubkey.clone(), relays.clone());
        let approved: Vec<BrowserDevice> = bunker
            .browsers
            .iter()
            .filter(|b| !b.blocked && b.approved_at.is_some())
            .cloned()
            .collect();
        let now = time::now_ms();
        for browser in &approved {
            handler::add_browser_connection(bunker, &pubkey, browser, now);
        }
        for connection in bunker
            .connections
            .iter_mut()
            .filter(|c| c.identity_pubkey == pubkey && c.browsers)
        {
            connection.relays = relays.clone();
        }
        Ok(())
    }

    /// Ends the browsers' session for an identity whose key goes back to the browsers.
    pub(crate) fn drop_browser_link(&mut self, identity_id: &str) -> Result<()> {
        let pubkey = self.find(identity_id)?.pubkey.clone();
        self.identity_mut(identity_id)?.browsers = None;
        let bunker = &mut self.unlocked_mut()?.data.bunker;
        let ended: Vec<String> = bunker
            .connections
            .iter()
            .filter(|c| c.browsers && c.identity_pubkey == pubkey)
            .map(|c| c.id.clone())
            .collect();
        bunker.connections.retain(|c| !ended.contains(&c.id));
        bunker.removed.extend(ended);
        bunker.browser_relays.remove(&pubkey);
        Ok(())
    }

    /// The relays and keys to listen on: only for identities that have an app or an open link,
    /// and only with the signer keys this desktop serves. Another Gooti Desktop answers on its own.
    pub fn bunker_listen(&self) -> Listen {
        let Ok(unlocked) = self.unlocked() else {
            return Listen::default();
        };
        let bunker = &unlocked.data.bunker;
        let desktop_id = unlocked.data.desktop_id.as_str();
        let now = time::now_ms();
        // A Desktop only identity waits for browsers that connect with their own key, so it is
        // in use even before any browser has.
        let desktop_only: Vec<&str> = unlocked
            .data
            .identities
            .iter()
            .filter(|i| i.browsers.is_some() && i.privkey.is_some())
            .map(|i| i.pubkey.as_str())
            .collect();
        let in_use = |identity: &str| {
            desktop_only.contains(&identity)
                || bunker
                    .connections
                    .iter()
                    .any(|c| c.identity_pubkey == identity)
                || bunker.invites.iter().any(|i| {
                    i.identity_pubkey == identity && now - i.created_at < INVITE_LIFETIME_MS
                })
        };
        let mut listen = Listen::default();
        for signer in &bunker.signers {
            if in_use(&signer.identity_pubkey)
                && signer.served_by(desktop_id)
                && let Ok(keys) = keys_from_hex(&signer.secret_hex)
            {
                listen.signer_pubkeys.push(keys.public_key().to_hex());
            }
        }
        if listen.signer_pubkeys.is_empty() {
            return listen;
        }
        // Every relay an app uses and every relay a waiting link names. Not the default relays:
        // nothing listens there until a link or an identity uses them.
        let mut relays: Vec<String> = Vec::new();
        let waiting = bunker
            .invites
            .iter()
            .filter(|i| now - i.created_at < INVITE_LIFETIME_MS)
            .flat_map(|i| invite_relays(bunker, i).iter());
        let used = bunker
            .connections
            .iter()
            .flat_map(|c| handler::app_relays(bunker, c).iter());
        // Where a Desktop only identity's pointer sends browsers that are not connected yet.
        let pointers: Vec<String> = desktop_only
            .iter()
            .flat_map(|pubkey| handler::browser_relays(bunker, pubkey))
            .collect();
        for relay in used.chain(waiting).chain(pointers.iter()) {
            if !relays.contains(relay) {
                relays.push(relay.clone());
            }
        }
        listen.relays = relays;
        listen
    }

    /// Opens and handles a request addressed to one of this desktop's signer keys.
    pub fn bunker_handle(
        &mut self,
        event: &nostr::event::Event,
        now: i64,
    ) -> Result<Option<Handled>> {
        let unlocked = self.unlocked_mut()?;
        let data = &mut unlocked.data;
        let targets: Vec<String> = event.tags.public_keys().map(|key| key.to_hex()).collect();
        let Some((signer, identity_pubkey)) = data.bunker.signers.iter().find_map(|signer| {
            let keys = keys_from_hex(&signer.secret_hex).ok()?;
            targets
                .contains(&keys.public_key().to_hex())
                .then(|| (keys, signer.identity_pubkey.clone()))
        }) else {
            return Ok(None);
        };
        let Some(incoming) = super::message::open_request(event, &signer) else {
            return Ok(None);
        };
        let identity = data.identities.iter().find(|i| i.pubkey == identity_pubkey);
        let has_key = identity.is_some_and(|i| i.privkey.is_some());
        // Browsers connect only to a Desktop only identity, with the secret its pointer carries.
        let browser_secret = identity
            .filter(|i| i.custody == Custody::Desktop)
            .and_then(|i| i.privkey.as_deref())
            .map(browser_secret);
        let before = data.bunker.connections.len();
        let before_app = data
            .bunker
            .connections
            .iter()
            .find(|c| {
                c.client_pubkey == incoming.client_pubkey && c.identity_pubkey == identity_pubkey
            })
            .map(|c| (app_name(c), c.id.clone()));
        // Read before handling: a logout removes the connection and its relays with it.
        // A new `bunker://` connection gets the bunker's relays either way.
        let reply_relays = reply_relays_for(&data.bunker, &incoming, &identity_pubkey);
        let (outcome, changed) = handler::handle(
            &mut data.bunker,
            &identity_pubkey,
            has_key,
            browser_secret.as_deref(),
            &signer.public_key().to_hex(),
            &incoming,
            now,
        );
        let connection_change = (data.bunker.connections.len() != before).then(|| {
            let app_of = |connections: &[Connection]| {
                connections
                    .iter()
                    .find(|c| {
                        c.client_pubkey == incoming.client_pubkey
                            && c.identity_pubkey == identity_pubkey
                    })
                    .map(|c| (app_name(c), c.id.clone()))
            };
            let unknown = || ("An app".to_string(), String::new());
            if data.bunker.connections.len() > before {
                let (app, connection_id) = app_of(&data.bunker.connections).unwrap_or_else(unknown);
                Activity::Connected {
                    app,
                    connection_id,
                    identity_pubkey: identity_pubkey.clone(),
                }
            } else {
                let (app, connection_id) = before_app.clone().unwrap_or_else(unknown);
                Activity::Disconnected {
                    app,
                    connection_id,
                    identity_pubkey: identity_pubkey.clone(),
                }
            }
        });
        if self.unlocked()?.data.desktop_record.is_none() {
            // Without a desktop record there is nobody to tell about a logout.
            self.unlocked_mut()?.data.bunker.removed.clear();
        }
        if changed {
            self.save()?;
        }
        Ok(Some(Handled {
            outcome,
            signer,
            reply_relays,
            client_pubkey: incoming.client_pubkey,
            scheme: incoming.scheme,
            connection_change,
        }))
    }

    /// Runs an approved request. The outer `Err` is a vault problem; the inner one goes to the app.
    pub fn bunker_perform(&self, ask: &Ask) -> Result<std::result::Result<String, String>> {
        let data = &self.unlocked()?.data;
        let Some(identity) = data
            .identities
            .iter()
            .find(|i| i.pubkey == ask.identity_pubkey)
        else {
            return Ok(Err("This identity is gone.".into()));
        };
        let keys = match identity.keys() {
            Ok(keys) => keys,
            Err(_) => return Ok(Err("This desktop cannot sign for this identity.".into())),
        };
        Ok(handler::perform(&keys, &ask.method, &ask.params))
    }

    /// The signer key of an identity this desktop serves, or else its first one.
    pub fn bunker_signer(&self, identity_pubkey: &str) -> Result<Option<Keys>> {
        let unlocked = self.unlocked()?;
        let signers = &unlocked.data.bunker.signers;
        let of_identity = || {
            signers
                .iter()
                .filter(|s| s.identity_pubkey == identity_pubkey)
        };
        Ok(of_identity()
            .find(|s| s.served_by(&unlocked.data.desktop_id))
            .or_else(|| of_identity().next())
            .and_then(|s| keys_from_hex(&s.secret_hex).ok()))
    }

    /// The key an approved request came to, so the answer goes out with the same key.
    pub fn bunker_signer_for_ask(&self, ask: &Ask) -> Result<Option<Keys>> {
        let found = self
            .unlocked()?
            .data
            .bunker
            .signers
            .iter()
            .filter_map(|s| keys_from_hex(&s.secret_hex).ok())
            .find(|keys| keys.public_key().to_hex() == ask.signer_pubkey);
        match found {
            Some(keys) => Ok(Some(keys)),
            None => self.bunker_signer(&ask.identity_pubkey),
        }
    }

    /// The signer keys the bunker listens with, for relays that ask for NIP-42 AUTH.
    pub fn bunker_signer_keys(&self) -> Result<Vec<Keys>> {
        let listening = self.bunker_listen().signer_pubkeys;
        Ok(self
            .unlocked()?
            .data
            .bunker
            .signers
            .iter()
            .filter_map(|s| keys_from_hex(&s.secret_hex).ok())
            .filter(|keys| listening.contains(&keys.public_key().to_hex()))
            .collect())
    }

    /// Where answers to this app go.
    pub fn bunker_reply_relays(
        &self,
        client_pubkey: &str,
        identity_pubkey: &str,
    ) -> Result<Vec<String>> {
        let bunker = &self.unlocked()?.data.bunker;
        Ok(bunker
            .connections
            .iter()
            .find(|c| c.client_pubkey == client_pubkey && c.identity_pubkey == identity_pubkey)
            .map(|c| handler::reply_relays(bunker, c))
            // A browser that is not connected yet listens where the identity's pointer says.
            .unwrap_or_else(|| handler::browser_relays(bunker, identity_pubkey)))
    }

    /// The public key of an identity this desktop can sign for.
    fn signable(&self, identity_id: &str) -> Result<String> {
        let identity = self.find(identity_id)?;
        if identity.privkey.is_none() {
            return Err(Error::CannotSign);
        }
        Ok(identity.pubkey.clone())
    }

    /// A signer key of an identity that this desktop serves, made on first use. A key another
    /// desktop serves is left to it: this desktop would not listen with it.
    fn signer_for(&mut self, identity_pubkey: &str) -> Result<Keys> {
        let unlocked = self.unlocked_mut()?;
        let desktop_id = unlocked.data.desktop_id.clone();
        let bunker = &mut unlocked.data.bunker;
        if let Some(signer) = bunker
            .signers
            .iter()
            .find(|s| s.identity_pubkey == identity_pubkey && s.served_by(&desktop_id))
        {
            return keys_from_hex(&signer.secret_hex);
        }
        let keys = Keys::generate();
        bunker.signers.push(SignerKey {
            identity_pubkey: identity_pubkey.to_string(),
            secret_hex: keys.secret_key().to_secret_hex(),
            served_by: None,
        });
        Ok(keys)
    }
}

/// The pointer backup carries for a Desktop only identity: this desktop's signer key, its `wss://`
/// relays, and the browsers' session key, so every browser can sign through it.
pub fn browser_pointer(
    bunker: &super::state::BunkerData,
    identity: &StoredIdentity,
) -> Option<SharedBunker> {
    let browsers = identity.browsers.as_ref()?;
    let mut of_identity = bunker
        .signers
        .iter()
        .filter(|s| s.identity_pubkey == identity.pubkey)
        .filter_map(|s| keys_from_hex(&s.secret_hex).ok());
    let signer = match &browsers.signer_pubkey {
        Some(named) => of_identity.find(|keys| keys.public_key().to_hex() == *named),
        None => of_identity.next(),
    }?;
    let relays: Vec<String> = handler::browser_relays(bunker, &identity.pubkey)
        .into_iter()
        .filter(|r| is_wss_url(r))
        .collect();
    if relays.is_empty() {
        return None;
    }
    Some(SharedBunker {
        pubkey: signer.public_key().to_hex(),
        relays,
        // What a browser's `connect` must carry. The backup seals it with the pointer.
        secret: identity.privkey.as_deref().map(browser_secret),
        ask_site: browsers.ask_site,
        // Browsers sign with their own key, which never goes into backup.
        client_secret_key: None,
        extra: Default::default(),
    })
}

/// The app's own name, or the start of its key.
pub fn app_name(connection: &Connection) -> String {
    connection.name.clone().unwrap_or_else(|| {
        format!(
            "App {}…",
            &connection.client_pubkey[..8.min(connection.client_pubkey.len())]
        )
    })
}

fn reply_relays_for(
    bunker: &super::state::BunkerData,
    incoming: &Incoming,
    identity_pubkey: &str,
) -> Vec<String> {
    bunker
        .connections
        .iter()
        .find(|c| c.client_pubkey == incoming.client_pubkey && c.identity_pubkey == identity_pubkey)
        .map(|c| handler::reply_relays(bunker, c))
        .unwrap_or_else(|| handler::browser_relays(bunker, identity_pubkey))
}

/// A waiting link's relays. Links made before 2026-11-12 named the default relays.
fn invite_relays<'a>(bunker: &'a super::state::BunkerData, invite: &'a Invite) -> &'a [String] {
    if invite.relays.is_empty() {
        &bunker.relays
    } else {
        &invite.relays
    }
}

/// `wss://` relays, plus `ws://` on this computer for a local relay.
fn normalize_bunker_relays(relays: &[String]) -> Result<Vec<String>> {
    let local: Vec<String> = relays
        .iter()
        .filter(|r| is_local_ws(r.trim()))
        .map(|r| r.trim().to_string())
        .collect();
    let remote: Vec<String> = relays
        .iter()
        .filter(|r| !is_local_ws(r.trim()))
        .cloned()
        .collect();
    let mut relays = if remote.iter().all(|r| r.trim().is_empty()) && !local.is_empty() {
        Vec::new()
    } else {
        normalize_relays(&remote)?
    };
    for relay in local {
        if !relays.contains(&relay) {
            relays.push(relay);
        }
    }
    Ok(relays)
}

fn is_local_ws(url: &str) -> bool {
    ["ws://localhost", "ws://127.0.0.1", "ws://[::1]"]
        .iter()
        .any(|prefix| {
            url.starts_with(prefix)
                && matches!(url.as_bytes().get(prefix.len()), None | Some(b':' | b'/'))
        })
}

pub(crate) fn keys_from_hex(secret_hex: &str) -> Result<Keys> {
    SecretKey::from_hex(secret_hex)
        .map(Keys::new)
        .map_err(|_| Error::UnreadableVault)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bunker::message::{Scheme, open_response, request_event, response_event};
    use crate::crypto::KdfParams;
    use crate::identity::NewKey;

    fn vault(dir: &tempfile::TempDir) -> Vault {
        let mut vault = Vault::with_kdf(dir.path().join("vault.json"), KdfParams::TEST);
        vault.create("1234").unwrap();
        vault
    }

    fn signer_pubkey(link: &str) -> String {
        link.trim_start_matches("bunker://")[..64].to_string()
    }

    fn secret(link: &str) -> String {
        link.split("secret=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .to_string()
    }

    fn reply_of(handled: Handled) -> serde_json::Value {
        match handled.outcome {
            Outcome::Reply(response) => serde_json::to_value(response).unwrap(),
            _ => panic!("expected a reply"),
        }
    }

    #[test]
    fn a_bunker_link_connects_an_app_and_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault(&dir);
        let alice = vault.add_identity("Alice", NewKey::Generate).unwrap();
        assert_eq!(vault.bunker_listen(), Listen::default());

        let link = vault.bunker_link(&alice.id).unwrap();
        assert!(link.starts_with("bunker://"));
        assert!(link.contains("relay=wss%3A%2F%2Frelay.getgooti.com"));
        let listen = vault.bunker_listen();
        assert_eq!(listen.signer_pubkeys, [signer_pubkey(&link)]);
        assert_ne!(
            listen.signer_pubkeys[0], alice.pubkey,
            "the signer key is not the identity key"
        );

        let app = Keys::generate();
        let connect = request_event(
            &app,
            &signer_pubkey(&link),
            "1",
            "connect",
            &[&signer_pubkey(&link), &secret(&link)],
            Scheme::Nip44,
        );
        let handled = vault
            .bunker_handle(&connect, time::now_ms())
            .unwrap()
            .unwrap();
        assert_eq!(reply_of(handled)["result"], "ack");

        vault.lock();
        vault.unlock("1234").unwrap();
        let info = vault.bunker_info().unwrap();
        assert_eq!(info.connections.len(), 1);
        assert_eq!(info.connections[0].identity_nick, "Alice");
        assert_eq!(info.connections[0].client_pubkey, app.public_key().to_hex());

        let ask_key = request_event(
            &app,
            &signer_pubkey(&link),
            "2",
            "get_public_key",
            &[],
            Scheme::Nip44,
        );
        let handled = vault
            .bunker_handle(&ask_key, time::now_ms())
            .unwrap()
            .unwrap();
        // The reply, as the app reads it.
        let event = response_event(
            &handled.signer,
            &app.public_key().to_hex(),
            &match handled.outcome {
                Outcome::Reply(r) => r,
                _ => panic!(),
            },
            Scheme::Nip44,
        )
        .unwrap();
        assert_eq!(open_response(&event, &app).unwrap()["result"], alice.pubkey);
    }

    /// Connects an app through a fresh bunker link and returns the app's key.
    fn connect_by_link(vault: &mut Vault, identity_id: &str) -> Keys {
        let link = vault.bunker_link(identity_id).unwrap();
        let app = Keys::generate();
        let connect = request_event(
            &app,
            &signer_pubkey(&link),
            "1",
            "connect",
            &[&signer_pubkey(&link), &secret(&link)],
            Scheme::Nip44,
        );
        vault
            .bunker_handle(&connect, time::now_ms())
            .unwrap()
            .unwrap();
        app
    }

    #[test]
    fn an_app_from_a_bunker_link_keeps_its_relays_when_the_list_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault(&dir);
        let alice = vault.add_identity("Alice", NewKey::Generate).unwrap();
        let app = connect_by_link(&mut vault, &alice.id);
        let first = vault.bunker_info().unwrap().relays;

        vault
            .bunker_set_relays(&["wss://new.example".into()])
            .unwrap();
        let info = vault.bunker_info().unwrap();
        assert_eq!(
            info.connections[0].relays, first,
            "the app keeps its link's relays"
        );
        let listen = vault.bunker_listen();
        assert_eq!(
            listen.relays, first,
            "listens where the app is, not on the new default"
        );
        assert_eq!(
            vault
                .bunker_reply_relays(&app.public_key().to_hex(), &alice.pubkey)
                .unwrap(),
            first
        );

        // A link made now names the new list.
        let later = connect_by_link(&mut vault, &alice.id);
        let info = vault.bunker_info().unwrap();
        let later = info
            .connections
            .iter()
            .find(|c| c.client_pubkey == later.public_key().to_hex())
            .unwrap();
        assert_eq!(later.relays, ["wss://new.example"]);
    }

    #[test]
    fn a_link_on_chosen_relays_waits_there_and_the_app_keeps_them() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault(&dir);
        let alice = vault.add_identity("Alice", NewKey::Generate).unwrap();
        let chosen = vec!["wss://chosen.example".to_string()];
        let link = vault.bunker_link_on(&alice.id, Some(&chosen)).unwrap();
        assert!(link.contains("relay=wss%3A%2F%2Fchosen.example"));
        assert!(
            !link.contains("relay.getgooti.com"),
            "not the default relays"
        );
        assert_eq!(vault.bunker_listen().relays, chosen);
        assert_eq!(vault.bunker_info().unwrap().link_relays, chosen);

        let app = Keys::generate();
        let connect = request_event(
            &app,
            &signer_pubkey(&link),
            "1",
            "connect",
            &[&signer_pubkey(&link), &secret(&link)],
            Scheme::Nip44,
        );
        vault
            .bunker_handle(&connect, time::now_ms())
            .unwrap()
            .unwrap();
        let info = vault.bunker_info().unwrap();
        assert_eq!(info.connections[0].relays, chosen);
        assert!(info.link_relays.is_empty(), "the link was used");
        assert!(matches!(
            vault.bunker_set_app_relays(&info.connections[0].id, &["wss://x.example".into()]),
            Err(Error::AppRelaysFixed)
        ));
    }

    #[test]
    fn activity_names_the_connection_of_each_app_entry() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault(&dir);
        let alice = vault.add_identity("Alice", NewKey::Generate).unwrap();
        let link = format!(
            "nostrconnect://{}?relay=wss://r.example&secret=x",
            Keys::generate().public_key().to_hex()
        );
        vault.bunker_accept_nostrconnect(&link, &alice.id).unwrap();
        let id = vault.bunker_info().unwrap().connections[0].id.clone();
        vault.bunker_revoke(&id).unwrap();
        let entries = vault.activity().unwrap();
        let ids: Vec<&str> = entries
            .iter()
            .filter_map(|e| match &e.what {
                Activity::Connected { connection_id, .. }
                | Activity::Disconnected { connection_id, .. } => Some(connection_id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ids, [id.as_str(), id.as_str()], "connected, then removed");
    }

    #[test]
    fn apps_that_kept_no_relays_get_todays_list_once_on_unlock() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault(&dir);
        let alice = vault.add_identity("Alice", NewKey::Generate).unwrap();
        connect_by_link(&mut vault, &alice.id);
        // As stored before 2026-11-12.
        vault.unlocked_mut().unwrap().data.bunker.connections[0]
            .relays
            .clear();
        vault.save().unwrap();
        let today = vault.bunker_info().unwrap().relays;

        let bob = vault
            .add_identity_as("Bob", NewKey::Generate, crate::identity::Custody::Desktop)
            .unwrap();
        // The shared key of Gooti 2.0 browsers, as stored before 2026-11-17, without relays.
        let shared = Keys::generate().public_key().to_hex();
        {
            let bunker = &mut vault.unlocked_mut().unwrap().data.bunker;
            bunker.browser_relays.clear();
            bunker.connections.push(Connection {
                id: "shared".into(),
                identity_pubkey: bob.pubkey.clone(),
                client_pubkey: shared.clone(),
                relays: Vec::new(),
                name: Some("Gooti browsers".into()),
                url: None,
                image: None,
                perms: None,
                created_at: 0,
                last_used_at: None,
                trust: Trust::Basic,
                rules: Vec::new(),
                browsers: true,
            });
        }
        vault.save().unwrap();

        vault.lock();
        vault.unlock("1234").unwrap();
        let bunker = &vault.unlocked().unwrap().data.bunker;
        assert_eq!(bunker.connections[0].relays, today);
        // The shared key is retired, and Bob's browsers keep today's relays, not the default.
        assert!(!bunker.connections.iter().any(|c| c.browsers));
        assert_eq!(bunker.retired_shared_keys, [shared]);
        assert_eq!(bunker.browser_relays.get(&bob.pubkey), Some(&today));
        vault
            .bunker_set_relays(&["wss://new.example".into()])
            .unwrap();
        assert_eq!(vault.bunker_info().unwrap().connections[0].relays, today);
    }

    #[test]
    fn nostrconnect_links_connect_on_the_apps_relays() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault(&dir);
        let alice = vault.add_identity("Alice", NewKey::Generate).unwrap();
        let app = Keys::generate();
        let link = format!(
            "nostrconnect://{}?relay=wss%3A%2F%2Fapp.relay&secret=abc&name=Snort",
            app.public_key().to_hex()
        );
        let reply = vault.bunker_accept_nostrconnect(&link, &alice.id).unwrap();
        assert_eq!(reply.secret, "abc");
        assert_eq!(reply.relays, ["wss://app.relay"]);
        let listen = vault.bunker_listen();
        assert!(listen.relays.contains(&"wss://app.relay".to_string()));
        assert_eq!(
            vault
                .bunker_reply_relays(&app.public_key().to_hex(), &alice.pubkey)
                .unwrap(),
            ["wss://app.relay"]
        );
        assert_eq!(
            vault.bunker_info().unwrap().connections[0].name.as_deref(),
            Some("Snort")
        );
    }

    #[test]
    fn revoke_pause_relays_and_deleting_the_identity() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault(&dir);
        let alice = vault.add_identity("Alice", NewKey::Generate).unwrap();
        let app = Keys::generate();
        let link = format!(
            "nostrconnect://{}?relay=wss://r.example&secret=x",
            app.public_key().to_hex()
        );
        vault.bunker_accept_nostrconnect(&link, &alice.id).unwrap();
        let id = vault.bunker_info().unwrap().connections[0].id.clone();

        vault.bunker_set_paused(true).unwrap();
        assert!(vault.bunker_info().unwrap().paused);
        assert_eq!(
            vault
                .bunker_set_relays(&[" wss://a.example ".into()])
                .unwrap(),
            ["wss://a.example"]
        );
        assert!(matches!(
            vault.bunker_set_relays(&["http://x".into()]),
            Err(Error::InvalidRelay)
        ));
        assert!(matches!(
            vault.bunker_set_relays(&["ws://relay.example".into()]),
            Err(Error::InvalidRelay)
        ));
        assert!(matches!(
            vault.bunker_set_relays(&["ws://localhost.evil.com".into()]),
            Err(Error::InvalidRelay)
        ));
        assert_eq!(
            vault
                .bunker_set_relays(&["ws://127.0.0.1:7777".into()])
                .unwrap(),
            ["ws://127.0.0.1:7777"]
        );
        assert!(matches!(vault.bunker_set_relays(&[]), Err(Error::NoRelays)));

        vault.bunker_revoke(&id).unwrap();
        assert!(vault.bunker_info().unwrap().connections.is_empty());
        assert!(matches!(
            vault.bunker_revoke(&id),
            Err(Error::ConnectionNotFound)
        ));

        vault.bunker_accept_nostrconnect(&link, &alice.id).unwrap();
        vault.delete_identity(&alice.id).unwrap();
        assert!(vault.bunker_info().unwrap().connections.is_empty());
        assert_eq!(vault.bunker_listen(), Listen::default());
    }

    #[test]
    fn bunker_identities_from_a_browser_cannot_get_links() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault(&dir);
        let alice = vault.add_identity("Alice", NewKey::Generate).unwrap();
        vault.unlocked_mut().unwrap().data.identities[0].privkey = None;
        assert!(matches!(
            vault.bunker_link(&alice.id),
            Err(Error::CannotSign)
        ));
    }

    #[test]
    fn approved_requests_use_the_identity_key() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault(&dir);
        let alice = vault.add_identity("Alice", NewKey::Generate).unwrap();
        let link = vault.bunker_link(&alice.id).unwrap();
        let app = Keys::generate();
        let signer = signer_pubkey(&link);
        vault
            .bunker_handle(
                &request_event(
                    &app,
                    &signer,
                    "1",
                    "connect",
                    &[&signer, &secret(&link)],
                    Scheme::Nip44,
                ),
                1,
            )
            .unwrap();
        let template = r#"{"kind":1,"content":"hi","tags":[],"created_at":1700000000}"#;
        let handled = vault
            .bunker_handle(
                &request_event(&app, &signer, "2", "sign_event", &[template], Scheme::Nip44),
                2,
            )
            .unwrap()
            .unwrap();
        let Outcome::Ask(ask) = handled.outcome else {
            panic!("expected a question")
        };
        let signed: nostr::event::Event =
            serde_json::from_str(&vault.bunker_perform(&ask).unwrap().unwrap()).unwrap();
        assert_eq!(signed.pubkey.to_hex(), alice.pubkey);
        assert_eq!(
            vault
                .bunker_signer(&alice.pubkey)
                .unwrap()
                .unwrap()
                .public_key()
                .to_hex(),
            signer
        );
    }

    #[test]
    fn events_for_other_keys_are_not_ours() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault(&dir);
        let app = Keys::generate();
        let event = request_event(
            &app,
            &Keys::generate().public_key().to_hex(),
            "1",
            "ping",
            &[],
            Scheme::Nip44,
        );
        assert!(vault.bunker_handle(&event, 1).unwrap().is_none());
    }
}
