//! What a backup changed, in words. Port of `backup/changes.ts`.
//! Lines name nicks and hosts. They never include keys or secrets.

use std::collections::HashMap;

use serde::Serialize;

use super::document::{
    SharedBunker, SharedDocument, SharedIdentity, SharedPermission, SharedSite, bunker_asks_site,
    js_number_text,
};
use super::merge::active_shared_document;

const CHANGE_LIMIT: usize = 80;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ChangeReport {
    /// Rows this device did not have before the merge.
    pub took: Vec<String>,
    /// Rows the relay did not have, and this backup sent.
    pub sent: Vec<String>,
}

/// Compares the vault this device held, the relay vault, and the merge.
/// A missing backup is one line, not a list of every row.
pub fn describe_changes(
    local: &SharedDocument,
    remote: Option<&SharedDocument>,
    merged: &SharedDocument,
    published: bool,
) -> ChangeReport {
    let Some(remote) = remote else {
        return ChangeReport {
            took: Vec::new(),
            sent: if published {
                vec!["Created the backup.".into()]
            } else {
                Vec::new()
            },
        };
    };
    let merged_active = active_shared_document(merged);
    let took = cap(diff_documents(
        &active_shared_document(local),
        &merged_active,
    ));
    let sent = if published {
        cap(diff_documents(
            &active_shared_document(remote),
            &merged_active,
        ))
    } else {
        Vec::new()
    };
    ChangeReport { took, sent }
}

fn diff_documents(before: &SharedDocument, after: &SharedDocument) -> Vec<String> {
    let names = nick_map(before, after);
    let mut lines = identity_lines(&before.identities, &after.identities);
    lines.extend(permission_lines(
        &before.permissions,
        &after.permissions,
        &names,
    ));
    lines.extend(site_lines(&before.sites, &after.sites, &names));
    lines
}

fn identity_lines(before: &[SharedIdentity], after: &[SharedIdentity]) -> Vec<String> {
    let previous: HashMap<&str, &SharedIdentity> =
        before.iter().map(|i| (i.pubkey.as_str(), i)).collect();
    let next = last_by_key(after, |i| i.pubkey.clone());
    let (mut added, mut removed, mut renamed, mut bunker) = (vec![], vec![], vec![], vec![]);
    for identity in &next {
        let name = name_of(&identity.nick);
        let Some(older) = previous.get(identity.pubkey.as_str()) else {
            added.push(if identity.bunker.is_some() {
                format!("Added bunker {name}")
            } else {
                format!("Added identity {name}")
            });
            continue;
        };
        if name_of(&older.nick) != name {
            renamed.push(format!("Renamed {} to {name}", name_of(&older.nick)));
        }
        if let Some(line) = bunker_line(older, identity) {
            bunker.push(line);
        }
    }
    let next_keys: Vec<&str> = next.iter().map(|i| i.pubkey.as_str()).collect();
    for identity in last_by_key(before, |i| i.pubkey.clone()) {
        if !next_keys.contains(&identity.pubkey.as_str()) {
            removed.push(format!("Removed identity {}", name_of(&identity.nick)));
        }
    }
    [
        sorted(added),
        sorted(removed),
        sorted(renamed),
        sorted(bunker),
    ]
    .concat()
}

fn bunker_line(before: &SharedIdentity, after: &SharedIdentity) -> Option<String> {
    let name = name_of(&after.nick);
    match (&before.bunker, &after.bunker) {
        (None, Some(_)) => Some(format!("Added a bunker for {name}")),
        (Some(_), None) => Some(format!("Removed the bunker from {name}")),
        (Some(a), Some(b)) if bunker_changed(a, b) => {
            Some(format!("Updated the bunker for {name}"))
        }
        _ => None,
    }
}

fn bunker_changed(before: &SharedBunker, after: &SharedBunker) -> bool {
    before.pubkey != after.pubkey
        || before.secret != after.secret
        || bunker_asks_site(Some(before)) != bunker_asks_site(Some(after))
        || before.relays.join("\n") != after.relays.join("\n")
}

fn permission_lines(
    before: &[SharedPermission],
    after: &[SharedPermission],
    names: &HashMap<String, String>,
) -> Vec<String> {
    let previous: HashMap<String, &SharedPermission> =
        before.iter().map(|p| (p.key(), p)).collect();
    let next = last_by_key(after, SharedPermission::key);
    let (mut added, mut removed, mut changed) = (vec![], vec![], vec![]);
    for permission in &next {
        let label = permission_label(permission, names);
        let Some(older) = previous.get(&permission.key()) else {
            added.push(if permission.method_policy == "deny" {
                format!("Added denial {label}")
            } else {
                format!("Added permission {label}")
            });
            continue;
        };
        if older.method_policy != permission.method_policy {
            changed.push(if permission.method_policy == "deny" {
                format!("Denied {label}")
            } else {
                format!("Allowed {label}")
            });
        }
    }
    let next_keys: Vec<String> = next.iter().map(|p| p.key()).collect();
    for permission in last_by_key(before, SharedPermission::key) {
        if !next_keys.contains(&permission.key()) {
            removed.push(format!(
                "Removed permission {}",
                permission_label(permission, names)
            ));
        }
    }
    [sorted(added), sorted(removed), sorted(changed)].concat()
}

fn site_lines(
    before: &[SharedSite],
    after: &[SharedSite],
    names: &HashMap<String, String>,
) -> Vec<String> {
    let previous: HashMap<&str, &SharedSite> =
        before.iter().map(|s| (s.host.as_str(), s)).collect();
    let next = last_by_key(after, |s| s.host.clone());
    let (mut added, mut removed, mut changed) = (vec![], vec![], vec![]);
    for site in &next {
        let who = names
            .get(&site.identity_pubkey)
            .cloned()
            .unwrap_or_else(|| "Unknown identity".into());
        match previous.get(site.host.as_str()) {
            None => added.push(format!("{} uses {who}", site.host)),
            Some(older) if older.identity_pubkey != site.identity_pubkey => {
                changed.push(format!("{} now uses {who}", site.host));
            }
            Some(_) => {}
        }
    }
    let next_hosts: Vec<&str> = next.iter().map(|s| s.host.as_str()).collect();
    for site in last_by_key(before, |s| s.host.clone()) {
        if !next_hosts.contains(&site.host.as_str()) {
            removed.push(format!("Cleared {}", site.host));
        }
    }
    [sorted(added), sorted(removed), sorted(changed)].concat()
}

fn permission_label(permission: &SharedPermission, names: &HashMap<String, String>) -> String {
    let kind = permission
        .kind
        .map(|kind| format!(" kind {}", js_number_text(kind)))
        .unwrap_or_default();
    let who = names
        .get(&permission.identity_pubkey)
        .cloned()
        .unwrap_or_else(|| "Unknown identity".into());
    format!("{} · {}{kind} · {who}", permission.host, permission.method)
}

fn nick_map(before: &SharedDocument, after: &SharedDocument) -> HashMap<String, String> {
    before
        .identities
        .iter()
        .chain(after.identities.iter())
        .map(|identity| (identity.pubkey.clone(), name_of(&identity.nick)))
        .collect()
}

fn name_of(nick: &str) -> String {
    let nick = nick.trim();
    if nick.is_empty() {
        "Unnamed".into()
    } else {
        nick.into()
    }
}

/// Rows by key, the last one winning, in first-seen order: like building a JavaScript `Map`.
fn last_by_key<T>(rows: &[T], key_of: impl Fn(&T) -> String) -> Vec<&T> {
    let mut order: Vec<String> = Vec::new();
    let mut map: HashMap<String, &T> = HashMap::new();
    for row in rows {
        let key = key_of(row);
        if map.insert(key.clone(), row).is_none() {
            order.push(key);
        }
    }
    order.iter().map(|key| map[key]).collect()
}

/// Close to JavaScript's `localeCompare` for these lines: case-insensitive first, then exact.
fn sorted(mut lines: Vec<String>) -> Vec<String> {
    lines.sort_by(|a, b| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
    lines
}

fn cap(lines: Vec<String>) -> Vec<String> {
    if lines.len() <= CHANGE_LIMIT {
        return lines;
    }
    let total = lines.len();
    let mut shown: Vec<String> = lines.into_iter().take(CHANGE_LIMIT - 1).collect();
    shown.push(format!("And {} more.", total - shown.len()));
    shown
}
