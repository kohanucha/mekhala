use super::*;

    #[test]
    fn test_display_wallet_not_found() {
        let err = NwcError::WalletNotFound;
        assert_eq!(err.to_string(), "Wallet not connected");
    }

    #[test]
    fn test_display_timeout() {
        let err = NwcError::Timeout;
        assert_eq!(err.to_string(), "NWC RPC timeout");
    }

    #[test]
    fn test_display_protocol_error() {
        let err = NwcError::ProtocolError("bad thing".into());
        assert_eq!(err.to_string(), "bad thing");
    }

    #[test]
    fn test_display_rpc_error() {
        let err = NwcError::RpcError { code: "PAYMENT_FAILED".into(), message: "no funds".into() };
        assert_eq!(err.to_string(), "NWC Error (PAYMENT_FAILED): no funds");
    }

    #[test]
    fn test_from_relay_error() {
        let relay_err = crate::nostr::RelayError::LimitExceeded("rate".into());
        let nwc: NwcError = relay_err.into();
        assert!(matches!(nwc, NwcError::ProtocolError(_)));
        assert!(nwc.to_string().contains("rejected"));
    }

    #[test]
    fn test_from_nwc_uri_error() {
        let uri_err = crate::nostr::nip_47::NwcUriError::InvalidScheme;
        let nwc: NwcError = uri_err.into();
        assert!(matches!(nwc, NwcError::ProtocolError(_)));
        assert!(nwc.to_string().contains("Invalid scheme"));
    }

    #[test]
    fn test_from_serde_error() {
        let serde_err: serde_json::Error = serde_json::from_str::<()>("invalid").unwrap_err();
        let nwc: NwcError = serde_err.into();
        assert!(matches!(nwc, NwcError::ProtocolError(_)));
    }

    #[test]
    fn test_nwc_error_debug() {
        let err = NwcError::Timeout;
        assert!(format!("{:?}", err).contains("Timeout"));
    }

    #[test]
    fn test_nwc_error_clone() {
        let err = NwcError::WalletNotFound;
        let cloned = err.clone();
        assert!(matches!(cloned, NwcError::WalletNotFound));
    }
