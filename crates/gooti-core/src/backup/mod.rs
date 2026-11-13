//! Backup over Nostr relays, compatible with the Gooti browser extension.
//! Each file ports the file of the same name in `gooti-extension/projects/common/src/lib/backup`.

pub mod blob;
pub mod canonical;
pub mod changes;
pub mod desktop;
pub mod document;
pub mod event;
pub mod flow;
pub mod merge;
pub mod phrase;
pub mod relay;
pub mod state;
