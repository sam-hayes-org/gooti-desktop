//! One small record per browser or desktop that opens a backup: its name, what it is, and when it
//! last backed up. Port of `backup/device.ts`.
//!
//! A replaceable kind 30078 event with `d` = `gooti-device:<id>`, signed and sealed with the shared
//! record's keys. Settings lists them as "Devices". "Remove from list" publishes a removed record;
//! a device that backs up again shows up again.

use nostr::event::Event;
use serde::Serialize;
use serde_json::{Value, json};

use super::blob::{open_plaintext, seal_plaintext};
use super::document::Millis;
use super::event::{BACKUP_KIND, build_backup_event};
use super::phrase::BackupKeys;
use crate::error::{Error, Result};

pub const DEVICE_D_PREFIX: &str = "gooti-device:";
/// At most this often after a backup, so most backups add no second event.
pub const DEVICE_REFRESH_MS: Millis = 60.0 * 60.0 * 1000.0;
const MAX_NAME: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceApp {
    Browser,
    Desktop,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRecord {
    pub id: String,
    pub name: String,
    pub app: DeviceApp,
    /// When this device last backed up, in ms.
    pub backed_up_at: Millis,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeviceEntry {
    Live(DeviceRecord),
    /// "Remove from list".
    Removed {
        id: String,
        removed_at: Millis,
    },
}

impl DeviceEntry {
    pub fn id(&self) -> &str {
        match self {
            DeviceEntry::Live(record) => &record.id,
            DeviceEntry::Removed { id, .. } => id,
        }
    }
}

pub fn new_device_id() -> String {
    crate::crypto::random_bytes::<16>()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 32 lower-case hex characters.
pub fn is_device_id(text: &str) -> bool {
    text.len() == 32
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// "Gooti Desktop on Linux". Each device can rename itself.
pub fn default_desktop_device_name() -> String {
    let system = match std::env::consts::OS {
        "linux" => "Linux",
        "windows" => "Windows",
        "macos" => "macOS",
        other => other,
    };
    format!("Gooti Desktop on {system}")
}

/// Trimmed, inner spaces collapsed, at most 80 characters. Empty is refused.
pub fn normalize_device_name(name: &str) -> Result<String> {
    let collapsed = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return Err(Error::NameRequired);
    }
    Ok(collapsed.chars().take(MAX_NAME).collect())
}

pub fn device_tag(id: &str) -> String {
    format!("{DEVICE_D_PREFIX}{id}")
}

pub fn seal_device(entry: &DeviceEntry, encryption_key: &[u8; 32]) -> Result<String> {
    let device = match entry {
        DeviceEntry::Live(record) => json!({
            "id": record.id,
            "name": record.name,
            "app": record.app,
            "backedUpAt": js_number(record.backed_up_at),
        }),
        DeviceEntry::Removed { id, removed_at } => {
            json!({ "id": id, "removedAt": js_number(*removed_at) })
        }
    };
    let plaintext = serde_json::to_vec(&json!({ "v": 1, "device": device }))
        .expect("a device record serializes");
    seal_plaintext(&plaintext, encryption_key)
}

/// `None` when the record cannot be opened or read.
pub fn open_device(payload: &str, encryption_key: &[u8; 32]) -> Option<DeviceEntry> {
    let plaintext = open_plaintext(payload, encryption_key).ok()?;
    read_device(&serde_json::from_slice(&plaintext).ok()?)
}

/// Same checks as the extension's `readDevice`.
pub fn read_device(value: &Value) -> Option<DeviceEntry> {
    if value.get("v") != Some(&Value::from(1)) {
        return None;
    }
    let device = value.get("device")?.as_object()?;
    let id = device.get("id")?.as_str().filter(|id| is_device_id(id))?;
    if let Some(removed) = device.get("removedAt") {
        return Some(DeviceEntry::Removed {
            id: id.into(),
            removed_at: removed.as_f64().filter(|n| n.is_finite())?,
        });
    }
    let name = device
        .get("name")?
        .as_str()
        .filter(|name| !name.trim().is_empty() && name.chars().count() <= MAX_NAME)?;
    let app = match device.get("app")?.as_str()? {
        "browser" => DeviceApp::Browser,
        "desktop" => DeviceApp::Desktop,
        _ => return None,
    };
    let backed_up_at = device
        .get("backedUpAt")?
        .as_f64()
        .filter(|n| n.is_finite())?;
    Some(DeviceEntry::Live(DeviceRecord {
        id: id.into(),
        name: name.into(),
        app,
        backed_up_at,
    }))
}

pub fn build_device_event(keys: &BackupKeys, id: &str, payload: &str, created_at: u64) -> Event {
    build_backup_event(keys, &device_tag(id), payload, created_at)
}

/// The device id and payload, after the signature, kind, author and d tag check out.
pub fn read_device_event<'a>(event: &'a Event, pubkey: &str) -> Option<(String, &'a str)> {
    let mut tags = event
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().map(String::as_str) == Some("d"));
    let tag = tags.next()?.as_slice().get(1)?.clone();
    if tags.next().is_some() {
        return None;
    }
    let id = tag.strip_prefix(DEVICE_D_PREFIX)?;
    let valid = is_device_id(id)
        && event.kind == nostr::event::Kind::Custom(BACKUP_KIND)
        && event.pubkey.to_hex() == pubkey
        && event.verify().is_ok();
    valid.then(|| (id.to_string(), event.content.as_str()))
}

/// The devices on the relays, newest record per device, newest backup first. A removed device
/// and a record whose id does not match its tag are left out.
pub fn devices_from_events(events: &[Event], keys: &BackupKeys) -> Vec<DeviceRecord> {
    let pubkey = keys.pubkey_hex();
    let mut newest: Vec<(String, &Event)> = Vec::new();
    for event in events {
        let Some((id, _)) = read_device_event(event, &pubkey) else {
            continue;
        };
        match newest.iter_mut().find(|(known, _)| *known == id) {
            Some(slot) if event.created_at > slot.1.created_at => slot.1 = event,
            Some(_) => {}
            None => newest.push((id, event)),
        }
    }
    let mut devices: Vec<DeviceRecord> = newest
        .into_iter()
        .filter_map(
            |(id, event)| match open_device(&event.content, &keys.encryption_key) {
                Some(DeviceEntry::Live(record)) if record.id == id => Some(record),
                _ => None,
            },
        )
        .collect();
    devices.sort_by(|a, b| {
        b.backed_up_at
            .total_cmp(&a.backed_up_at)
            .then_with(|| a.name.cmp(&b.name))
    });
    devices
}

/// Whole milliseconds as integers, like JavaScript writes them.
fn js_number(n: Millis) -> Value {
    if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 {
        Value::from(n as i64)
    } else {
        Value::from(n)
    }
}
