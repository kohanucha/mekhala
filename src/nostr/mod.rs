//! Nostr protocol machinery of the relay.
//!
//! The protocol types themselves live in the `nostr-core` crate, shared with
//! NWC clients; they are re-exported here so relay code keeps one path.
//!
//! - [`engine`]: relay state machine on top of the [`wallet_registry`]
//! - [`wallet_registry`]: subscription registry with DO-storage persistence
//! - everything else: re-exported from `nostr_core`

pub mod engine;
pub mod wallet_registry;

pub use nostr_core::{limits, nip_01, nip_44, nip_47, rpc_machine};
pub use nostr_core::{
    ClientMessage, Event, Filter, Limits, RelayError, RelayMessage, Result, WalletInfo,
};
pub use wallet_registry::StorageError;

// Only the relay's tests build tags directly.
#[cfg(test)]
pub use nostr_core::Tag;
