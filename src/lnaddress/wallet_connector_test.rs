use super::*;
    use crate::nostr::WalletInfo;
    use crate::common::test_helpers::{TEST_WALLET_SK, TEST_NWC_URI};
    use crate::common::test_helpers::MockTransport;

    #[test]
    fn test_session_make_invoice() {
        futures::executor::block_on(async {
            let wallet_info = WalletInfo {
                encryption_algorithms: vec![EncryptionMethod::Nip04],
            };

            let uri_obj = NwcUri::from_uri(TEST_NWC_URI).unwrap();
            let app_client = NwcClient::new(uri_obj).unwrap();
            let wallet_uri = NwcUri {
                wallet_pubkey: app_client.my_pubkey.clone(),
                secret: TEST_WALLET_SK.into(),
            };

            let transport = MockTransport {
                wallet_info: Some(wallet_info),
                wallet_uri: Some(wallet_uri),
                error_code: None,
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
                wallet_pubkey: app_client.my_pubkey.clone(),
                secret: TEST_WALLET_SK.into(),
            };

            let transport = MockTransport {
                wallet_info: Some(wallet_info),
                wallet_uri: Some(wallet_uri),
                error_code: None,
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
                wallet_pubkey: app_client.my_pubkey.clone(),
                secret: TEST_WALLET_SK.into(),
            };

            let transport = MockTransport {
                wallet_info: Some(wallet_info),
                wallet_uri: Some(wallet_uri),
                error_code: Some("INSUFFICIENT_BALANCE".into()),
            };

            let session = NwcSession::new(&transport, TEST_NWC_URI).unwrap();
            let result = session.make_invoice(1000, "hash".into()).await;

            assert!(result.is_err());
            match result.err().unwrap() {
                NwcError::RpcError { code, message } => {
                    assert_eq!(code, "INSUFFICIENT_BALANCE");
                    assert_eq!(message, "insufficient balance");
                }
                other => panic!("Expected RpcError, got {:?}", other),
            }
        });
    }
