use std::fmt;

/// Error type for the entire relay domain: event validation, NIP protocol
/// handling, crypto failures, and limit enforcement.
///
/// String payloads carry human-readable detail (e.g. offending values or
/// upstream error text) that is forwarded as Nostr `OK`/`NOTICE` messages,
/// so they are deliberately not machine-readable tags.
#[derive(Debug, PartialEq, Clone)]
pub enum RelayError {
    /// Event kind is not on the relay's allow list.
    InvalidKind,
    /// Event timestamp lies outside the relay's accepted window.
    TimestampTooFar(String),
    /// A required Nostr tag (`e`/`p`) is missing for an NWC request.
    MissingTag(String),
    /// Recomputed event ID does not match the `id` in the event.
    InvalidId,
    /// Schnorr signature verification failed.
    InvalidSignature,
    /// Hex decoding failed for the given field.
    MalformedHex(String),
    /// JSON serialization or deserialization failed.
    SerializationError(String),
    /// The request exceeds a configured relay limit.
    LimitExceeded(String),
    /// A cryptographic operation (key, ECDH, signature) failed.
    CryptoError(String),
    /// Base64 decoding failed.
    Base64Error(String),
    /// UTF-8 decoding failed.
    Utf8Error(String),
    /// Anything not covered by the specific variants.
    Generic(String),
}

/// Result alias used across the relay domain.
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

#[cfg(test)]
#[path = "error_test.rs"]
mod error_test;