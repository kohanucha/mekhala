pub mod wallet_registry;
pub mod nip_01;
pub mod nip_04;
pub mod nip_44;
pub mod nip_47;
pub mod engine;
pub mod event;
pub mod filter;

pub use nip_01::{RelayMessage, ClientMessage};
pub use event::Event;
pub use filter::Filter;

use crate::util::engine::Engine;
use crate::nostr::engine::NostrEngine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

pub fn create_engine() -> Box<dyn Engine> {
    Box::new(NostrEngine::new())
}

pub fn get_nip_11_info() -> serde_json::Value {
    serde_json::json!({
        "supported_nips": [1, 11, 47]
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Limits {
    pub max_filter_items: usize,
    pub max_event_tags: usize,
    pub max_content_length: usize,
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

#[derive(Serialize, Deserialize, Clone)]
pub struct ConnectionState {
    pub id: u32,
    pub subscriptions: HashMap<String, Vec<Filter>>,
    pub info_event: Option<Event>,
    pub limits: Limits,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self {
            id: 0,
            subscriptions: HashMap::new(),
            info_event: None,
            limits: Limits::default(),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, PartialEq, Clone)]
pub enum RelayError {
    InvalidKind,
    TimestampTooFar(String),
    MissingTag(String),
    InvalidId,
    InvalidSignature,
    MalformedHex(String),
    SerializationError(String),
    ParseError(String),
    LimitExceeded(String),
}

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
            Self::ParseError(e) => write!(f, "error: parse failed: {}", e),
            Self::LimitExceeded(m) => write!(f, "rejected: {}", m),
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

use k256::{PublicKey as K256PublicKey, SecretKey as K256SecretKey};
use worker::{Error, Result};

pub fn get_shared_secret(secret_key_hex: &str, public_key_hex: &str) -> Result<Vec<u8>> {
    let secret_key_bytes = hex::decode(secret_key_hex).map_err(|e| Error::from(e.to_string()))?;
    let sk =
        K256SecretKey::from_slice(&secret_key_bytes).map_err(|e| Error::from(e.to_string()))?;

    let public_key_bytes = hex::decode(public_key_hex).map_err(|e| Error::from(e.to_string()))?;
    let mut full_pk_bytes = [0u8; 33];
    full_pk_bytes[0] = 0x02;
    full_pk_bytes[1..].copy_from_slice(&public_key_bytes);

    let pk =
        K256PublicKey::from_sec1_bytes(&full_pk_bytes).map_err(|e| Error::from(e.to_string()))?;

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
    fn test_connection_state_default() {
        let state = ConnectionState::default();
        assert_eq!(state.id, 0);
        assert!(state.subscriptions.is_empty());
        assert!(state.info_event.is_none());
        assert_eq!(state.limits.max_filter_items, 100);
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
        assert_eq!(RelayError::ParseError("err".into()).to_string(), "error: parse failed: err");
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
