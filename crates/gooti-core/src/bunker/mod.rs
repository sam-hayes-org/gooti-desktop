//! A NIP-46 remote signer ("bunker"). Apps connect with a `bunker://` link from this desktop or
//! by handing it a `nostrconnect://` link; requests travel as kind 24133 events over relays.

pub mod handler;
pub mod message;
pub mod policy;
pub mod pool;
pub mod service;
pub mod state;
pub mod store;
pub mod uri;
