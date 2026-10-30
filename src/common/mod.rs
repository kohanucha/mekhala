//! Shared types: `NwcError`, `NwcTransport` trait, `UserStore` trait.

use std::fmt;
use crate::nostr::Event;
use crate::nostr::nip_47::NwcUriError;

#[derive(Debug, Clone)]
/// Error type for the wallet-service side of NWC interactions.
pub enum NwcError {
    /// No wallet session is connected for the requested user.
    WalletNotFound,
    /// The wallet did not answer in time.
    Timeout,
    /// NWC-level failure; carries the upstream message.
    ProtocolError(String),
    /// The wallet returned a structured NIP-47 error.
    RpcError {
        /// NIP-47 machine-readable code.
        code: String,
        /// Human-readable message from the wallet.
        message: String,
    },
}

impl fmt::Display for NwcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NwcError::WalletNotFound => write!(f, "Wallet not connected"),
            NwcError::Timeout => write!(f, "NWC RPC timeout"),
            NwcError::ProtocolError(msg) => write!(f, "{}", msg),
            NwcError::RpcError { code, message } => write!(f, "NWC Error ({}): {}", code, message),
        }
    }
}

impl From<crate::nostr::RelayError> for NwcError {
    fn from(e: crate::nostr::RelayError) -> Self {
        NwcError::ProtocolError(e.to_string())
    }
}

impl From<NwcUriError> for NwcError {
    fn from(e: NwcUriError) -> Self {
        NwcError::ProtocolError(e.to_string())
    }
}

impl From<serde_json::Error> for NwcError {
    fn from(e: serde_json::Error) -> Self {
        NwcError::ProtocolError(e.to_string())
    }
}

#[async_trait::async_trait(?Send)]
/// Boundary the LN address gateway uses to talk to a live wallet connection.
/// Implemented by the Cloudflare Durable Object transport.
pub trait NwcTransport {
    /// Returns the wallet's advertised capabilities, or `None` when offline.
    async fn get_wallet_info(&self, pubkey: &str) -> Option<crate::nostr::WalletInfo>;

    /// Sends a signed NIP-47 request event and awaits the wallet's response
    /// event.
    ///
    /// # Errors
    ///
    /// Returns [`NwcError`] for timeouts, protocol failures, and NWC-level
    /// errors reported by the wallet.
    async fn execute_nwc_rpc(&self, request: Event) -> Result<Event, NwcError>;
}

#[async_trait::async_trait(?Send)]
/// Lookup of a user's stored NWC connection URI (backed by KV / mocks).
pub trait UserStore {
    /// The `nostr+walletconnect://` URI stored for `username`, if connected.
    async fn get_nwc_uri(&self, username: &str) -> Option<String>;
}

#[cfg(test)]
#[path = "test_helpers.rs"]
pub(crate) mod test_helpers;

#[cfg(test)]
#[path = "common_test.rs"]
mod common_test;

