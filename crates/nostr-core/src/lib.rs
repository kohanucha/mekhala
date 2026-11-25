//! Nostr protocol types shared by the relay and by NWC clients.
//!
//! Nothing here depends on a runtime: the relay Worker (wasm32) and a native
//! service both build this crate, so it holds only the protocol itself.
//!
//! - [`event`], [`tag`], [`filter`]: NIP-01 core types (event validation,
//!   typed tag decoding, subscription filters)
//! - [`nip_01`]: wire messages between client and relay
//! - [`nip_04`], [`nip_44`]: content encryption schemes
//! - [`nip_47`]: NWC request/response construction and parsing
//! - [`rpc_machine`]: NWC request/response state machine
//! - [`error`], [`limits`]: shared error type and protocol ceilings
//! - [`clock`]: wall-clock seconds on both targets

pub mod clock;
pub mod error;
pub mod event;
pub mod filter;
pub mod limits;
pub mod nip_01;
pub mod nip_04;
pub mod nip_44;
pub mod nip_47;
pub mod rpc_machine;
pub mod tag;

pub use error::{RelayError, Result};
pub use event::Event;
pub use filter::Filter;
pub use limits::Limits;
pub use nip_01::{ClientMessage, RelayMessage};
pub use nip_47::WalletInfo;
pub use tag::Tag;

#[cfg(test)]
mod test_helpers;
