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
mod tests {
    use super::*;
    use base64::Engine;

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

    #[test]
    fn test_relay_error_display_missing_variants() {
        assert_eq!(RelayError::CryptoError("bad".into()).to_string(), "error: crypto failure: bad");
        assert_eq!(RelayError::Base64Error("b64 bad".into()).to_string(), "error: base64 failure: b64 bad");
        assert_eq!(RelayError::Utf8Error("utf8 bad".into()).to_string(), "error: utf8 failure: utf8 bad");
        assert_eq!(RelayError::Generic("oops".into()).to_string(), "error: oops");
    }

    #[test]
    fn test_relay_error_from_base64() {
        let b64_err = base64::engine::general_purpose::STANDARD.decode("!!!").unwrap_err();
        let relay_err: RelayError = b64_err.into();
        match relay_err {
            RelayError::Base64Error(msg) => assert!(!msg.is_empty()),
            _ => panic!("Expected Base64Error"),
        }
    }
}