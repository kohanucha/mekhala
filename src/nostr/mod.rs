pub mod nip_01;
pub mod nip_04;
pub mod nip_44;
pub mod nip_47;
pub mod engine;
pub mod wallet_registry;
pub mod event;
pub mod filter;
pub mod rpc_machine;

pub use nip_01::{RelayMessage, ClientMessage};
pub use event::Event;
pub use filter::Filter;


use serde::{Deserialize, Serialize};
use std::fmt;

pub fn get_nip_11_info() -> serde_json::Value {
    serde_json::json!({
        "supported_nips": [1, 11, 47]
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WalletInfo {
    pub online: bool,
    pub ready: bool,
    pub encryption_algorithms: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Limits {
    pub max_filter_items: usize,
    pub max_event_tags: usize,
    pub max_content_length: usize,
}

impl Limits {
    pub fn new(max_filter_items: usize, max_event_tags: usize, max_content_length: usize) -> Self {
        Self {
            max_filter_items,
            max_event_tags,
            max_content_length,
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_filter_items: 100,
            max_event_tags: 100,
            max_content_length: 32768,
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum RelayError {
    InvalidKind,
    TimestampTooFar(String),
    MissingTag(String),
    InvalidId,
    InvalidSignature,
    MalformedHex(String),
    SerializationError(String),
    LimitExceeded(String),
    CryptoError(String),
    Base64Error(String),
    Utf8Error(String),
    UrlError(String),
    Generic(String),
}

pub type Result<T> = std::result::Result<T, RelayError>;

impl fmt::Display for RelayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKind => write!(f, "blocked: event kind not allowed"),
            Self::TimestampTooFar(m) => write!(f, "invalid: {}", m),
            Self::MissingTag(m) => write!(f, "invalid: missing {}", m),
            Self::InvalidId => write!(f, "invalid: event ID mismatch"),
            Self::InvalidSignature => write!(f, "invalid: signature verification failed"),
            Self::MalformedHex(m) => write!(f, "invalid: malformed {}", m),
            Self::SerializationError(e) => write!(f, "error: serialization failed: {}", e),
            Self::LimitExceeded(m) => write!(f, "rejected: {}", m),
            Self::CryptoError(m) => write!(f, "error: crypto failure: {}", m),
            Self::Base64Error(m) => write!(f, "error: base64 failure: {}", m),
            Self::Utf8Error(m) => write!(f, "error: utf8 failure: {}", m),
            Self::UrlError(m) => write!(f, "error: url failure: {}", m),
            Self::Generic(m) => write!(f, "error: {}", m),
        }
    }
}

impl From<serde_json::Error> for RelayError {
    fn from(e: serde_json::Error) -> Self {
        Self::SerializationError(e.to_string())
    }
}

impl From<hex::FromHexError> for RelayError {
    fn from(e: hex::FromHexError) -> Self {
        Self::MalformedHex(e.to_string())
    }
}

impl From<base64::DecodeError> for RelayError {
    fn from(e: base64::DecodeError) -> Self {
        Self::Base64Error(e.to_string())
    }
}

use k256::{PublicKey as K256PublicKey, SecretKey as K256SecretKey};

pub fn get_shared_secret(secret_key_hex: &str, public_key_hex: &str) -> Result<Vec<u8>> {
    let secret_key_bytes = hex::decode(secret_key_hex).map_err(|e| RelayError::MalformedHex(e.to_string()))?;
    let sk =
        K256SecretKey::from_slice(&secret_key_bytes).map_err(|e| RelayError::CryptoError(e.to_string()))?;

    let public_key_bytes = hex::decode(public_key_hex).map_err(|e| RelayError::MalformedHex(e.to_string()))?;
    let mut full_pk_bytes = [0u8; 33];
    full_pk_bytes[0] = 0x02;
    full_pk_bytes[1..].copy_from_slice(&public_key_bytes);

    let pk =
        K256PublicKey::from_sec1_bytes(&full_pk_bytes).map_err(|e| RelayError::CryptoError(e.to_string()))?;

    let shared = k256::ecdh::diffie_hellman(sk.to_nonzero_scalar(), pk.as_affine());
    Ok(shared.raw_secret_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_limits_default() {
        let limits = Limits::default();
        assert_eq!(limits.max_filter_items, 100);
        assert_eq!(limits.max_event_tags, 100);
        assert_eq!(limits.max_content_length, 32768);
    }

    #[test]
    fn test_get_nip_11_info() {
        let info = get_nip_11_info();
        assert!(info.is_object());
        let nips = info.get("supported_nips").and_then(|v| v.as_array()).unwrap();
        assert!(nips.contains(&serde_json::json!(1)));
        assert!(nips.contains(&serde_json::json!(11)));
        assert!(nips.contains(&serde_json::json!(47)));
    }

    #[test]
    fn test_relay_error_display() {
        assert_eq!(RelayError::InvalidKind.to_string(), "blocked: event kind not allowed");
        assert_eq!(RelayError::TimestampTooFar("skew".into()).to_string(), "invalid: skew");
        assert_eq!(RelayError::MissingTag("p".into()).to_string(), "invalid: missing p");
        assert_eq!(RelayError::InvalidId.to_string(), "invalid: event ID mismatch");
        assert_eq!(RelayError::InvalidSignature.to_string(), "invalid: signature verification failed");
        assert_eq!(RelayError::MalformedHex("key".into()).to_string(), "invalid: malformed key");
        assert_eq!(RelayError::SerializationError("err".into()).to_string(), "error: serialization failed: err");
        assert_eq!(RelayError::LimitExceeded("max".into()).to_string(), "rejected: max");
    }

    #[test]
    fn test_relay_error_from_serde() {
        let json_err = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let relay_err: RelayError = json_err.into();
        match relay_err {
            RelayError::SerializationError(msg) => assert!(msg.contains("EOF")),
            _ => panic!("Expected SerializationError"),
        }
    }

    #[test]
    fn test_relay_error_from_hex() {
        let hex_err = hex::decode("invalid").unwrap_err();
        let relay_err: RelayError = hex_err.into();
        match relay_err {
            RelayError::MalformedHex(msg) => assert!(msg.contains("Odd number of digits")),
            _ => panic!("Expected MalformedHex"),
        }
    }
}
