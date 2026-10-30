//! Nostr protocol machinery of the relay.
//!
//! - [`event`], [`tag`], [`filter`]: NIP-01 core types (event validation,
//!   typed tag decoding, subscription filters)
//! - [`nip_01`]: wire messages between client and relay
//! - [`nip_04`], [`nip_44`]: content encryption schemes
//! - [`nip_47`]: NWC request/response construction and parsing
//! - [`engine`]: relay state machine on top of the [`wallet_registry`]
//! - [`wallet_registry`]: subscription registry with DO-storage persistence
//! - [`rpc_machine`]: NWC RPC request/response state machine
//! - [`error`], [`limits`]: shared error type and relay ceilings

pub mod nip_01;
pub mod nip_04;
pub mod nip_44;
pub mod nip_47;
pub mod engine;
pub mod wallet_registry;
pub mod event;
pub mod filter;
pub mod rpc_machine;
pub mod tag;
pub mod error;
pub mod limits;

pub use nip_01::{RelayMessage, ClientMessage};
pub use event::Event;
pub use filter::Filter;
pub use nip_47::WalletInfo;
pub use tag::Tag;
pub use error::{RelayError, Result};
pub use limits::Limits;
