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
