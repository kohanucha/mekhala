/// Error type for the entire relay domain: event validation, NIP protocol
/// handling, crypto failures, and limit enforcement.
///
/// String payloads carry human-readable detail (e.g. offending values or
/// upstream error text) that is forwarded as Nostr `OK`/`NOTICE` messages,
/// so they are deliberately not machine-readable tags.
#[derive(Debug, PartialEq, Clone, thiserror::Error)]
pub enum RelayError {
    /// Event kind is not on the relay's allow list.
    #[error("blocked: event kind not allowed")]
    InvalidKind,
    /// Event timestamp lies outside the relay's accepted window.
    #[error("invalid: {0}")]
    TimestampTooFar(String),
    /// A required Nostr tag (`e`/`p`) is missing for an NWC request.
    #[error("invalid: missing {0}")]
    MissingTag(String),
    /// Recomputed event ID does not match the `id` in the event.
    #[error("invalid: event ID mismatch")]
    InvalidId,
    /// Schnorr signature verification failed.
    #[error("invalid: signature verification failed")]
    InvalidSignature,
    /// Hex decoding failed for the given field.
    #[error("invalid: malformed {0}")]
    MalformedHex(String),
    /// JSON serialization or deserialization failed.
    #[error("error: serialization failed: {0}")]
    SerializationError(String),
    /// The request exceeds a configured relay limit.
    #[error("rejected: {0}")]
    LimitExceeded(String),
    /// A cryptographic operation (key, ECDH, signature) failed.
    #[error("error: crypto failure: {0}")]
    CryptoError(String),
    /// Base64 decoding failed.
    #[error("error: base64 failure: {0}")]
    Base64Error(String),
    /// UTF-8 decoding failed.
    #[error("error: utf8 failure: {0}")]
    Utf8Error(String),
    /// Anything not covered by the specific variants.
    #[error("error: {0}")]
    Generic(String),
}

/// Result alias used across the relay domain.
pub type Result<T> = std::result::Result<T, RelayError>;

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