//! The replaceable kind 30078 event that carries the blob. Port of `backup/event.ts`.

use nostr::event::{Event, EventBuilder, FinalizeEvent, Kind, Tag};
use nostr::types::Timestamp;

use super::phrase::BackupKeys;
use crate::error::{Error, Result};

pub const BACKUP_KIND: u16 = 30078;
/// The shared record: browsers and desktops read it.
pub const SHARED_D_TAG: &str = "gooti-shared";
/// The desktop record: only Gooti Desktops read it.
pub const DESKTOP_D_TAG: &str = "gooti-desktop";

pub fn build_backup_event(keys: &BackupKeys, d_tag: &str, payload: &str, created_at: u64) -> Event {
    EventBuilder::new(Kind::Custom(BACKUP_KIND), payload)
        .tag(Tag::identifier(d_tag))
        .custom_created_at(Timestamp::from_secs(created_at))
        .finalize(&keys.signing)
        .expect("signing with a local key cannot fail")
}

/// The blob payload, after the signature, the kind and the d tag check out.
pub fn read_backup_payload<'a>(event: &'a Event, d_tag: &str) -> Result<&'a str> {
    let tagged = event.tags.identifier().as_deref() == Some(d_tag);
    if event.verify().is_err() || event.kind != Kind::Custom(BACKUP_KIND) || !tagged {
        return Err(Error::InvalidBackup);
    }
    Ok(&event.content)
}
