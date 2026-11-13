//! What happened on this desktop: bunker requests and backups. Sealed in the vault, never backed up,
//! newest 500 kept. Locking keeps it; Reset deletes it with the vault.

use serde::{Deserialize, Serialize};

use crate::bunker::policy::Reason;

pub const ACTIVITY_LIMIT: usize = 500;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    /// Milliseconds.
    pub at: i64,
    #[serde(flatten)]
    pub what: Activity,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Activity {
    /// A bunker request that was signed, encrypted, decrypted, or refused.
    Request {
        app: String,
        /// The app's connection, for its own list on the app page. Empty in entries written before
        /// 2026-11-13: they show only in the full list.
        #[serde(default)]
        connection_id: String,
        identity_pubkey: String,
        method: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<u16>,
        approved: bool,
        reason: Reason,
        /// Set when an approved request still failed, such as a message that would not decrypt.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        /// The site, for requests from Gooti browsers that name it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        site: Option<String>,
    },
    Connected {
        app: String,
        /// The app's connection, for its own list on the app page. Empty in entries written before
        /// 2026-11-13: they show only in the full list.
        #[serde(default)]
        connection_id: String,
        identity_pubkey: String,
    },
    Disconnected {
        app: String,
        /// The app's connection, for its own list on the app page. Empty in entries written before
        /// 2026-11-13: they show only in the full list.
        #[serde(default)]
        connection_id: String,
        identity_pubkey: String,
    },
    /// A backup: "Back up now" or an automatic one.
    #[serde(alias = "sync")]
    Backup {
        automatic: bool,
        /// `backedUp`, `updated`, `current` or `failed` (`synced` in entries before 2026-11-12).
        result: String,
        #[serde(default)]
        took: Vec<String>,
        #[serde(default)]
        sent: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

/// Adds entries and keeps the newest `ACTIVITY_LIMIT`.
pub fn push(log: &mut Vec<ActivityEntry>, entry: ActivityEntry) {
    log.push(entry);
    if log.len() > ACTIVITY_LIMIT {
        let extra = log.len() - ACTIVITY_LIMIT;
        log.drain(..extra);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_entries() {
        let mut log = Vec::new();
        for at in 0..(ACTIVITY_LIMIT as i64 + 10) {
            push(
                &mut log,
                ActivityEntry {
                    at,
                    what: Activity::Connected {
                        app: "a".into(),
                        connection_id: "c".into(),
                        identity_pubkey: "p".into(),
                    },
                },
            );
        }
        assert_eq!(log.len(), ACTIVITY_LIMIT);
        assert_eq!(log[0].at, 10);
    }

    #[test]
    fn serializes_flat_for_the_ui() {
        let entry = ActivityEntry {
            at: 5,
            what: Activity::Request {
                app: "Coracle".into(),
                connection_id: "c1".into(),
                identity_pubkey: "p".into(),
                method: "sign_event".into(),
                kind: Some(1),
                approved: true,
                reason: Reason::Remembered,
                error: None,
                site: None,
            },
        };
        assert_eq!(
            serde_json::to_value(&entry).unwrap(),
            serde_json::json!({
                "at": 5, "type": "request", "app": "Coracle", "connectionId": "c1", "identityPubkey": "p",
                "method": "sign_event", "kind": 1, "approved": true, "reason": "remembered"
            })
        );
    }

    #[test]
    fn entries_without_a_connection_still_open() {
        let entry: ActivityEntry = serde_json::from_value(serde_json::json!({
            "at": 1, "type": "connected", "app": "Coracle", "identityPubkey": "p"
        }))
        .unwrap();
        assert!(
            matches!(entry.what, Activity::Connected { connection_id, .. } if connection_id.is_empty())
        );
    }
}
