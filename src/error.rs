use std::fmt;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_invalid_kind() {
        let err = RelayError::InvalidKind;
        assert_eq!(err.to_string(), "blocked: event kind not allowed");
    }

    #[test]
    fn test_display_timestamp_too_far() {
        let err = RelayError::TimestampTooFar("clock skew".into());
        assert_eq!(err.to_string(), "invalid: clock skew");
    }

    #[test]
    fn test_display_missing_tag() {
        let err = RelayError::MissingTag("p tag".into());
        assert_eq!(err.to_string(), "invalid: missing p tag");
    }

    #[test]
    fn test_display_invalid_id() {
        let err = RelayError::InvalidId;
        assert_eq!(err.to_string(), "invalid: event ID mismatch");
    }

    #[test]
    fn test_display_invalid_signature() {
        let err = RelayError::InvalidSignature;
        assert_eq!(err.to_string(), "invalid: signature verification failed");
    }

    #[test]
    fn test_display_malformed_hex() {
        let err = RelayError::MalformedHex("pubkey".into());
        assert_eq!(err.to_string(), "invalid: malformed pubkey");
    }

    #[test]
    fn test_display_serialization_error() {
        let err = RelayError::SerializationError("json error".into());
        assert_eq!(err.to_string(), "error: serialization failed: json error");
    }

    #[test]
    fn test_display_parse_error() {
        let err = RelayError::ParseError("invalid json".into());
        assert_eq!(err.to_string(), "error: parse failed: invalid json");
    }

    #[test]
    fn test_display_limit_exceeded() {
        let err = RelayError::LimitExceeded("too many filters".into());
        assert_eq!(err.to_string(), "rejected: too many filters");
    }

    #[test]
    fn test_from_serde_json_error() {
        let json_err = serde_json::from_str::<()>("invalid").unwrap_err();
        let err: RelayError = json_err.into();
        assert!(matches!(err, RelayError::SerializationError(_)));
    }

    #[test]
    fn test_from_hex_error() {
        let hex_err = hex::decode("zzz").unwrap_err();
        let err: RelayError = hex_err.into();
        assert!(matches!(err, RelayError::MalformedHex(_)));
    }
}