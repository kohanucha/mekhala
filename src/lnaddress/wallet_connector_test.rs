use super::*;
use crate::common::test_helpers::MockTransport;
use crate::common::test_helpers::{TEST_NWC_URI, TEST_WALLET_PK, TEST_WALLET_SK};
use crate::nostr::WalletInfo;

#[test]
fn test_session_make_invoice() {
    futures::executor::block_on(async {
        let wallet_info = WalletInfo {
            encryption_algorithms: vec![EncryptionMethod::Nip04],
        };

        let uri_obj = NwcUri::from_uri(TEST_NWC_URI).unwrap();
        let app_client = NwcClient::new(uri_obj).unwrap();
        let wallet_uri = NwcUri {
            wallet_pubkey: app_client.my_pubkey,
            secret: TEST_WALLET_SK.into(),
        };

        let transport = MockTransport {
            wallet_info: Some(wallet_info),
            wallet_uri: Some(wallet_uri),
            error_code: None,
            response_payload: None,
        };

        let session = NwcSession::new(&transport, TEST_NWC_URI).unwrap();
        let result = session.make_invoice(1000, "hash".into()).await;

        assert!(result.is_ok(), "RPC call failed: {:?}", result.err());
        assert_eq!(result.unwrap(), "lnbc1test");
    });
}

#[test]
fn test_session_encryption_negotiation() {
    futures::executor::block_on(async {
        let wallet_info = WalletInfo {
            encryption_algorithms: vec![EncryptionMethod::Nip44],
        };

        let uri_obj = NwcUri::from_uri(TEST_NWC_URI).unwrap();
        let app_client = NwcClient::new(uri_obj).unwrap();
        let wallet_uri = NwcUri {
            wallet_pubkey: app_client.my_pubkey,
            secret: TEST_WALLET_SK.into(),
        };

        let transport = MockTransport {
            wallet_info: Some(wallet_info),
            wallet_uri: Some(wallet_uri),
            error_code: None,
            response_payload: None,
        };

        let session = NwcSession::new(&transport, TEST_NWC_URI).unwrap();
        let result = session.make_invoice(1000, "hash".into()).await;

        assert!(result.is_ok(), "NIP-44 RPC call failed: {:?}", result.err());
        assert_eq!(result.unwrap(), "lnbc1test");
    });
}

#[test]
fn test_session_wallet_error() {
    futures::executor::block_on(async {
        let wallet_info = WalletInfo {
            encryption_algorithms: vec![EncryptionMethod::Nip04],
        };

        let uri_obj = NwcUri::from_uri(TEST_NWC_URI).unwrap();
        let app_client = NwcClient::new(uri_obj).unwrap();
        let wallet_uri = NwcUri {
            wallet_pubkey: app_client.my_pubkey,
            secret: TEST_WALLET_SK.into(),
        };

        let transport = MockTransport {
            wallet_info: Some(wallet_info),
            wallet_uri: Some(wallet_uri),
            error_code: Some("INSUFFICIENT_BALANCE".into()),
            response_payload: None,
        };

        let session = NwcSession::new(&transport, TEST_NWC_URI).unwrap();
        let result = session.make_invoice(1000, "hash".into()).await;

        assert!(result.is_err());
        match result.err().unwrap() {
            NwcError::RpcError { code, message } => {
                assert_eq!(code, "INSUFFICIENT_BALANCE");
                assert_eq!(message, "insufficient balance");
            }
            other => panic!("Expected RpcError, got {other:?}"),
        }
    });
}

/// NIP-04 transport whose RPC response payload is fully controlled.
fn transport_with_response(response_payload: Option<serde_json::Value>) -> MockTransport {
    let wallet_info = WalletInfo {
        encryption_algorithms: vec![EncryptionMethod::Nip04],
    };
    let uri_obj = NwcUri::from_uri(TEST_NWC_URI).unwrap();
    let app_client = NwcClient::new(uri_obj).unwrap();
    let wallet_uri = NwcUri {
        wallet_pubkey: app_client.my_pubkey,
        secret: TEST_WALLET_SK.into(),
    };
    MockTransport {
        wallet_info: Some(wallet_info),
        wallet_uri: Some(wallet_uri),
        error_code: None,
        response_payload,
    }
}

#[test]
fn test_session_new_rejects_invalid_secret() {
    let transport = MockTransport::wallet_not_found();
    let uri = format!("nostr+walletconnect://{TEST_WALLET_PK}?secret=zz");
    let err = NwcSession::new(&transport, &uri)
        .err()
        .expect("invalid secret must fail session creation");
    assert!(matches!(err, NwcError::ProtocolError(_)), "{err:?}");
}

#[test]
fn test_session_wallet_not_found() {
    futures::executor::block_on(async {
        let transport = MockTransport::wallet_not_found();
        let session = NwcSession::new(&transport, TEST_NWC_URI).unwrap();
        let err = session
            .make_invoice(1000, "hash".into())
            .await
            .expect_err("offline wallet must fail");
        assert!(matches!(err, NwcError::WalletNotFound), "{err:?}");
    });
}

#[test]
fn test_session_missing_invoice_in_response() {
    futures::executor::block_on(async {
        let transport = transport_with_response(Some(serde_json::json!({
            "result": {"fee": 1},
            "error": null
        })));
        let session = NwcSession::new(&transport, TEST_NWC_URI).unwrap();
        let err = session
            .make_invoice(1000, "hash".into())
            .await
            .expect_err("response without invoice must fail");
        match err {
            NwcError::ProtocolError(msg) => assert!(msg.contains("Missing invoice"), "{msg}"),
            other => panic!("expected ProtocolError, got {other:?}"),
        }
    });
}

#[test]
fn test_session_missing_result_and_error() {
    futures::executor::block_on(async {
        let transport = transport_with_response(Some(serde_json::json!({
            "result": null,
            "error": null
        })));
        let session = NwcSession::new(&transport, TEST_NWC_URI).unwrap();
        let err = session
            .make_invoice(1000, "hash".into())
            .await
            .expect_err("empty response must fail");
        match err {
            NwcError::ProtocolError(msg) => {
                assert!(msg.contains("missing result and error"), "{msg}");
            }
            other => panic!("expected ProtocolError, got {other:?}"),
        }
    });
}
