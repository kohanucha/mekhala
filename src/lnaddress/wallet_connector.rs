use crate::common::{NwcError, NwcTransport};
use crate::nostr::nip_47::{
    EncryptionMethod, NwcClient, NwcMethod, NwcResponse, NwcUri,
};
use serde_json::Value;

pub struct NwcSession<'a, T: NwcTransport> {
    transport: &'a T,
    client: NwcClient,
}

impl<'a, T: NwcTransport> NwcSession<'a, T> {
    pub fn new(transport: &'a T, nwc_uri: &str) -> Result<Self, NwcError> {
        let uri = NwcUri::from_uri(nwc_uri)?;
        let client = NwcClient::new(uri)?;
        Ok(Self { transport, client })
    }

    pub async fn make_invoice(&self, amount_msat: u64, description_hash: String) -> Result<String, NwcError> {
        let params = serde_json::json!({
            "amount": amount_msat,
            "description_hash": description_hash,
        });

        let result = self.call(NwcMethod::MakeInvoice, params).await?;

        result
            .get("invoice")
            .and_then(|i| i.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| NwcError::ProtocolError("Missing invoice in response".into()))
    }

    pub async fn call(&self, method: NwcMethod, params: Value) -> Result<Value, NwcError> {
        let client = self.negotiate_encryption().await?;
        let (event, request_id) = client.create_request_event(method, params, vec![])?;

        let resp_event = self.transport.execute_nwc_rpc(event).await?;

        let resp_json = client
            .parse_response_event(&resp_event, &request_id)?;

        let response: NwcResponse =
            serde_json::from_value(resp_json)?;

        if let Some(error) = response.error {
            return Err(NwcError::RpcError {
                code: error.code,
                message: error.message,
            });
        }

        response
            .result
            .ok_or_else(|| NwcError::ProtocolError("NWC response missing result and error".into()))
    }

    async fn negotiate_encryption(&self) -> Result<NwcClient, NwcError> {
        let mut client = self.client.clone();

        let info = self.transport
            .get_wallet_info(&client.wallet_pubkey)
            .await
            .ok_or(NwcError::WalletNotFound)?;

        if info.encryption_algorithms.contains(&EncryptionMethod::Nip44) {
            client.encryption_method = EncryptionMethod::Nip44;
        } else {
            client.encryption_method = EncryptionMethod::Nip04;
        };

        Ok(client)
    }
}

#[cfg(test)]
mod tests {
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
}