use super::*;
    use crate::nostr::{Event, Tag};
    use crate::nostr::WalletInfo;

    struct MockTransport {
        wallet_info: WalletInfo,
        wallet_uri: NwcUri,
        error_code: Option<String>,
    }

    #[async_trait::async_trait(?Send)]
    impl NwcTransport for MockTransport {
        async fn get_wallet_info(&self, _pubkey: &str) -> Option<WalletInfo> {
            Some(self.wallet_info.clone())
        }
        async fn execute_nwc_rpc(&self, request: Event) -> Result<Event, NwcError> {
            let mut wallet_client = NwcClient::new(self.wallet_uri.clone()).unwrap();
            
            let is_nip44 = request.tags.iter().any(|t| t.encryption_scheme() == Some("nip44_v2"));
            if is_nip44 {
                wallet_client.encryption_method = EncryptionMethod::Nip44;
            }

            let _ = wallet_client.decrypt(&request.content).map_err(NwcError::from)?;

            let resp_payload = if let Some(code) = &self.error_code {
                serde_json::json!({
                    "error": {
                        "code": code,
                        "message": "insufficient balance"
                    }
                })
            } else {
                serde_json::json!({
                    "result": {
                        "invoice": "lnbc1test"
                    }
                })
            };
            
            let encrypted = wallet_client.encrypt(&resp_payload).unwrap();
            let mut tags = vec![
                Tag::p(&wallet_client.my_pubkey),
                Tag::E(request.id.clone(), vec![]),
            ];
            if is_nip44 {
                tags.push(Tag::encryption("nip44_v2"));
            }
            
            let response_event = wallet_client.create_event(23195, encrypted, tags).unwrap();
            Ok(response_event)
        }
    }

    #[test]
    fn test_session_make_invoice() {
        futures::executor::block_on(async {
            let wallet_pk = "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f";
            let nwc_uri = format!("nostr+walletconnect://{}?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101", wallet_pk);
            
            let wallet_info = WalletInfo {
                encryption_algorithms: vec![EncryptionMethod::Nip04],
            };
            
            let uri_obj = NwcUri::from_uri(&nwc_uri).unwrap();
            let app_client = NwcClient::new(uri_obj).unwrap();
            let wallet_uri = NwcUri {
                wallet_pubkey: app_client.my_pubkey.clone(),
                secret: "0101010101010101010101010101010101010101010101010101010101010101".into(),
            };
            
            let transport = MockTransport {
                wallet_info,
                wallet_uri,
                error_code: None,
            };
            
            let session = NwcSession::new(&transport, &nwc_uri).unwrap();
            let result = session.make_invoice(1000, "hash".into()).await;
            
            assert!(result.is_ok(), "RPC call failed: {:?}", result.err());
            assert_eq!(result.unwrap(), "lnbc1test");
        });
    }

    #[test]
    fn test_session_encryption_negotiation() {
        futures::executor::block_on(async {
            let wallet_pk = "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f";
            let nwc_uri = format!("nostr+walletconnect://{}?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101", wallet_pk);
            
            let wallet_info = WalletInfo {
                encryption_algorithms: vec![EncryptionMethod::Nip44],
            };
            
            let uri_obj = NwcUri::from_uri(&nwc_uri).unwrap();
            let app_client = NwcClient::new(uri_obj).unwrap();
            let wallet_uri = NwcUri {
                wallet_pubkey: app_client.my_pubkey.clone(),
                secret: "0101010101010101010101010101010101010101010101010101010101010101".into(),
            };
            
            let transport = MockTransport {
                wallet_info,
                wallet_uri,
                error_code: None,
            };
            
            let session = NwcSession::new(&transport, &nwc_uri).unwrap();
            let result = session.make_invoice(1000, "hash".into()).await;
            
            assert!(result.is_ok(), "NIP-44 RPC call failed: {:?}", result.err());
            assert_eq!(result.unwrap(), "lnbc1test");
        });
    }

    #[test]
    fn test_session_wallet_error() {
        futures::executor::block_on(async {
            let wallet_pk = "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f";
            let nwc_uri = format!("nostr+walletconnect://{}?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101", wallet_pk);
            
            let wallet_info = WalletInfo {
                encryption_algorithms: vec![EncryptionMethod::Nip04],
            };
            
            let uri_obj = NwcUri::from_uri(&nwc_uri).unwrap();
            let app_client = NwcClient::new(uri_obj).unwrap();
            let wallet_uri = NwcUri {
                wallet_pubkey: app_client.my_pubkey.clone(),
                secret: "0101010101010101010101010101010101010101010101010101010101010101".into(),
            };
            
            let transport = MockTransport {
                wallet_info,
                wallet_uri,
                error_code: Some("INSUFFICIENT_BALANCE".into()),
            };
            
            let session = NwcSession::new(&transport, &nwc_uri).unwrap();
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
