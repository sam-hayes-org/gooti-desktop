//! The backup state sealed in the vault, and how it meets the live identities.
//! Port of the identity parts of `backup/state.ts`.
//!
//! The desktop holds no permissions or site bindings of its own: those rows belong to browsers.
//! They stay in the stored document as they are, so the desktop never drops them.

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use super::canonical::{canonical_shared_document, published_bunker};
use super::document::{
    Millis, Revisioned, SharedBunker, SharedDocument, SharedIdentity, is_hex_key,
};
use crate::color::color_for_pubkey;

/// Gooti's relay. Same default as the extension.
pub const DEFAULT_BACKUP_RELAYS: [&str; 1] = ["wss://relay.getgooti.com"];

/// Sealed inside the vault. The published blob never contains this object.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupState {
    pub signing_key_hex: String,
    pub encryption_key_hex: String,
    pub relays: Vec<String>,
    /// Active rows and tombstones. Live identities overlay the active rows when publishing.
    pub document: SharedDocument,
    /// This desktop only. Not part of the published blob.
    #[serde(default)]
    pub auto_backup: bool,
}

impl Drop for BackupState {
    fn drop(&mut self) {
        self.signing_key_hex.zeroize();
        self.encryption_key_hex.zeroize();
    }
}

/// An identity as this desktop holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveIdentity {
    pub pubkey: String,
    pub privkey: Option<String>,
    pub nick: String,
    pub created_at: String,
    pub color: Option<String>,
    pub bunker: Option<SharedBunker>,
    /// When this desktop last moved the key (see `SharedIdentity::custody_at`). `None` keeps the
    /// time the stored row has.
    pub custody_at: Option<Millis>,
}

/// Builds the document to publish from the live identities plus stored stamps.
/// A missing identity stamp is `Date.parse(createdAt)`, once. Later calls keep that stamp.
pub fn project_shared_document(
    stored: &SharedDocument,
    live: &[LiveIdentity],
    now: Millis,
) -> (SharedDocument, bool) {
    let mut identities = Vec::new();
    for identity in live {
        let stamp = stored
            .identities
            .iter()
            .find(|s| s.pubkey == identity.pubkey);
        let fallback = parse_js_date(&identity.created_at).unwrap_or(now);
        let resurrected = stamp.is_some_and(|s| s.deleted_at.is_some());
        let linked = conversation_key_added(identity, stamp);
        let updated_at = if resurrected || linked {
            now
        } else {
            stamp.map(|s| s.updated_at).unwrap_or(fallback)
        };
        identities.push(identity_record(identity, updated_at, stamp));
    }
    for stamp in &stored.identities {
        if stamp.deleted_at.is_some() && !live.iter().any(|i| i.pubkey == stamp.pubkey) {
            identities.push(stamp.clone());
        }
    }
    let document = SharedDocument {
        v: 2,
        identities,
        permissions: stored.permissions.clone(),
        relays: Vec::new(),
        sites: stored.sites.clone(),
    };
    let changed = canonical_shared_document(&document) != canonical_shared_document(stored);
    (document, changed)
}

/// Records an added or edited identity at `now`.
pub fn stamp_identity(document: &mut SharedDocument, identity: &LiveIdentity, now: Millis) {
    let previous = take_identity(document, &identity.pubkey);
    document
        .identities
        .push(identity_record(identity, now, previous.as_ref()));
}

/// Marks a deleted identity, its permissions and its site bindings.
/// The row keeps its key material, like the extension's tombstone.
pub fn tombstone_identity(document: &mut SharedDocument, identity: &LiveIdentity, now: Millis) {
    let previous = take_identity(document, &identity.pubkey);
    let updated_at = previous.as_ref().map(|p| p.updated_at).unwrap_or(now);
    let mut record = identity_record(identity, updated_at, previous.as_ref());
    record.deleted_at = Some(now);
    document.identities.push(record);
    document.relays.clear();

    for permission in document
        .permissions
        .iter_mut()
        .filter(|p| p.identity_pubkey == identity.pubkey && p.is_active())
    {
        permission.deleted_at = Some(now);
    }
    for site in document
        .sites
        .iter_mut()
        .filter(|s| s.identity_pubkey == identity.pubkey && s.is_active())
    {
        site.deleted_at = Some(now);
    }
}

/// The identities to keep after a merge. A public key already on this desktop keeps its id;
/// a new one gets `new_id()`. Order: this desktop's identities first, then new ones.
pub fn apply_shared_document<T>(
    local: &[T],
    merged: &SharedDocument,
    local_pubkey: impl Fn(&T) -> &str,
    local_bunker: impl Fn(&T) -> Option<&SharedBunker>,
    local_id: impl Fn(&T) -> String,
    mut new_id: impl FnMut() -> String,
) -> Vec<(String, LiveIdentity)> {
    let active: Vec<&SharedIdentity> = merged.identities.iter().filter(|i| i.is_active()).collect();
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for existing in local {
        let pubkey = local_pubkey(existing);
        let Some(winner) = active.iter().find(|w| w.pubkey == pubkey) else {
            continue;
        };
        seen.push(pubkey.to_string());
        out.push((
            local_id(existing),
            live_from(winner, local_bunker(existing)),
        ));
    }
    for winner in active {
        if !seen.contains(&winner.pubkey) {
            out.push((new_id(), live_from(winner, None)));
        }
    }
    out
}

fn live_from(winner: &SharedIdentity, previous_bunker: Option<&SharedBunker>) -> LiveIdentity {
    let bunker = winner.bunker.as_ref().map(|bunker| {
        let mut published = published_bunker(bunker);
        // A device that already has its own conversation key keeps it while the bunker matches.
        if let Some(previous) = previous_bunker
            && previous.pubkey == bunker.pubkey
            && previous.secret == bunker.secret
            && let Some(key) = &previous.client_secret_key
        {
            published.client_secret_key = Some(key.clone());
        }
        published
    });
    LiveIdentity {
        pubkey: winner.pubkey.clone(),
        privkey: winner.privkey.clone(),
        nick: winner.nick.clone(),
        created_at: winner.created_at.clone(),
        color: Some(color_for_pubkey(&winner.pubkey, winner.color.as_deref())),
        bunker,
        custody_at: winner.custody_at,
    }
}

fn identity_record(
    identity: &LiveIdentity,
    updated_at: Millis,
    previous: Option<&SharedIdentity>,
) -> SharedIdentity {
    let stored_color = identity
        .color
        .as_deref()
        .or_else(|| previous.and_then(|p| p.color.as_deref()));
    let bunker = identity.bunker.as_ref().or_else(|| {
        if identity.privkey.is_none() {
            previous.and_then(|p| p.bunker.as_ref())
        } else {
            None
        }
    });
    SharedIdentity {
        pubkey: identity.pubkey.clone(),
        privkey: identity.privkey.clone(),
        nick: identity.nick.clone(),
        color: Some(color_for_pubkey(&identity.pubkey, stored_color)),
        created_at: identity.created_at.clone(),
        updated_at,
        deleted_at: None,
        bunker: bunker.map(published_bunker),
        custody_at: identity
            .custody_at
            .or_else(|| previous.and_then(|p| p.custody_at)),
        extra: Default::default(),
    }
}

/// Publishing a key the stored stamp does not have must win over another device's older copy.
fn conversation_key_added(identity: &LiveIdentity, stamp: Option<&SharedIdentity>) -> bool {
    let live_key = identity
        .bunker
        .as_ref()
        .and_then(|b| b.client_secret_key.as_deref())
        .is_some_and(is_hex_key);
    let stamped = stamp
        .and_then(|s| s.bunker.as_ref())
        .is_some_and(|b| b.client_secret_key.is_some());
    live_key && !stamped
}

fn take_identity(document: &mut SharedDocument, pubkey: &str) -> Option<SharedIdentity> {
    let at = document
        .identities
        .iter()
        .position(|i| i.pubkey == pubkey)?;
    let previous = document.identities.remove(at);
    // `filter` in the extension drops every row with this key, not only the first.
    document.identities.retain(|i| i.pubkey != pubkey);
    Some(previous)
}

/// `Date.parse` for the ISO strings Gooti writes (`toISOString`), with or without milliseconds,
/// `Z` or `±hh:mm`. Anything else is `None`, and the caller uses now.
pub fn parse_js_date(text: &str) -> Option<Millis> {
    let text = text.trim();
    let bytes = text.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    let num = |range: std::ops::Range<usize>| -> Option<i64> { text.get(range)?.parse().ok() };
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let mut rest = &text[19..];
    let mut millis = 0i64;
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits: String = fraction.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            return None;
        }
        millis = format!("{digits:0<3}")[..3].parse().ok()?;
        rest = &fraction[digits.len()..];
    }
    let offset_minutes = match rest {
        "Z" => 0,
        _ if rest.len() == 6 && (rest.starts_with('+') || rest.starts_with('-')) => {
            let sign = if rest.starts_with('-') { -1 } else { 1 };
            sign * (rest[1..3].parse::<i64>().ok()? * 60 + rest[4..6].parse::<i64>().ok()?)
        }
        _ => return None,
    };
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 24
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month as u32, day as u32);
    let seconds = days * 86_400 + hour * 3600 + minute * 60 + second - offset_minutes * 60;
    Some((seconds * 1000 + millis) as f64)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(pubkey: &str, nick: &str) -> LiveIdentity {
        LiveIdentity {
            pubkey: pubkey.into(),
            privkey: None,
            nick: nick.into(),
            created_at: "2026-01-02T03:04:05.678Z".into(),
            color: None,
            bunker: Some(SharedBunker {
                pubkey: "ab".repeat(32),
                relays: vec!["wss://r".into()],
                secret: None,
                ask_site: None,
                client_secret_key: None,
                extra: Default::default(),
            }),
            custody_at: None,
        }
    }

    #[test]
    fn parses_iso_dates_like_javascript() {
        // Date.parse('2026-01-02T03:04:05.678Z')
        assert_eq!(
            parse_js_date("2026-01-02T03:04:05.678Z"),
            Some(1_767_323_045_678.0)
        );
        assert_eq!(
            parse_js_date("2026-01-02T03:04:05Z"),
            Some(1_767_323_045_000.0)
        );
        assert_eq!(
            parse_js_date("2026-01-02T05:04:05.678+02:00"),
            Some(1_767_323_045_678.0)
        );
        assert_eq!(parse_js_date("yesterday"), None);
    }

    #[test]
    fn a_new_stamp_starts_at_created_at_and_is_kept_later() {
        let alice = live(&"11".repeat(32), "Alice");
        let (first, changed) = project_shared_document(
            &SharedDocument::default(),
            std::slice::from_ref(&alice),
            9e12,
        );
        assert!(changed);
        assert_eq!(first.identities[0].updated_at, 1_767_323_045_678.0);
        let (second, changed) = project_shared_document(&first, &[alice], 9e12);
        assert!(!changed);
        assert_eq!(second, first);
    }

    #[test]
    fn projection_fills_the_color_from_the_pubkey() {
        let (document, _) = project_shared_document(
            &SharedDocument::default(),
            &[live(&"00".repeat(32), "A")],
            1.0,
        );
        assert_eq!(
            document.identities[0].color.as_deref(),
            Some(crate::color::color_from_pubkey(&"00".repeat(32)).as_str())
        );
    }

    #[test]
    fn tombstone_keeps_earlier_revision_and_marks_rows() {
        let alice = live(&"11".repeat(32), "Alice");
        let mut document = SharedDocument::default();
        stamp_identity(&mut document, &alice, 100.0);
        tombstone_identity(&mut document, &alice, 200.0);
        let row = &document.identities[0];
        assert_eq!((row.updated_at, row.deleted_at), (100.0, Some(200.0)));
        // A deleted identity that is not live stays as a tombstone in the next projection.
        let (projected, _) = project_shared_document(&document, &[], 300.0);
        assert_eq!(projected.identities.len(), 1);
        // Adding it again resurrects it at now.
        let (projected, _) = project_shared_document(&document, &[alice], 300.0);
        assert_eq!(projected.identities[0].updated_at, 300.0);
        assert_eq!(projected.identities[0].deleted_at, None);
    }
}
