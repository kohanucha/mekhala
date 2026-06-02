use std::fmt;

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