use std::fmt;
use crate::nostr::Event;
use crate::nostr::nip_47::NwcUriError;

#[derive(Debug, Clone)]
pub enum NwcError {
    WalletNotFound,
    Timeout,
    ProtocolError(String),
    RpcError { code: String, message: String },
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
pub trait NwcTransport {
    async fn get_wallet_info(&self, pubkey: &str) -> Option<crate::nostr::WalletInfo>;

    async fn execute_nwc_rpc(&self, request: Event) -> Result<Event, NwcError>;
}