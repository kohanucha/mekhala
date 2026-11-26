//! Nostr protocol machinery of the relay.
//!
//! - [`event`], [`tag`], [`filter`]: NIP-01 core types (event validation,
//!   typed tag decoding, subscription filters)
//! - [`nip_01`]: wire messages between client and relay
//! - [`engine`]: relay state machine on top of the [`wallet_registry`]
//! - [`wallet_registry`]: subscription registry with DO-storage persistence
//! - [`error`], [`limits`]: shared error type and relay ceilings
//!
//! NIP-47 content is never decrypted here: the relay checks signatures and
//! routes by tags, so it needs no NWC client or encryption code.

pub mod engine;
pub mod error;
pub mod event;
pub mod filter;
pub mod limits;
pub mod nip_01;
pub mod tag;
pub mod wallet_registry;

pub use error::{RelayError, Result, StorageError};
pub use event::Event;
pub use filter::Filter;
pub use limits::Limits;
pub use nip_01::{ClientMessage, RelayMessage};
pub use tag::Tag;
