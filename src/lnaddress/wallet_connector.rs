use worker::*;
use crate::nostr::nip_47::{WalletConnectionDetails, WalletConnection, EncryptionMethod};
use crate::common::InternalTransport;
use serde_json::Value;

pub struct WalletConnector {
    nwc_uri: String,
}

impl WalletConnector {
    pub fn new(_env: &Env, nwc_uri: &str) -> Self {
        Self {
            nwc_uri: nwc_uri.to_string(),
        }
    }

    pub async fn make_invoice(&self, transport: &impl InternalTransport, amount_msat: u64, description_hash: String) -> Result<String> {
        let connection_details = WalletConnectionDetails::from_uri(&self.nwc_uri)?;
        let mut connection = WalletConnection::new(connection_details)?;

        // 1. Check wallet capability via relay get_wallet_info
        let info = transport.get_wallet_info(&connection.wallet_pubkey).await
            .ok_or_else(|| Error::from("Wallet not connected"))?;
        
        if !info.online {
            return Err(Error::from("Wallet not connected"));
        }

        if !info.ready {
            return Err(Error::from("Wallet service info not ready"));
        }

        let encryption_algorithms = info.encryption_algorithms;

        // 2. Negotiate encryption: Prefer NIP-44, fallback to NIP-04
        let chosen_scheme = if encryption_algorithms.contains(&"nip44_v2".to_string()) {
            connection.encryption_method = EncryptionMethod::Nip44;
            "nip44_v2"
        } else {
            connection.encryption_method = EncryptionMethod::Nip04;
            "nip04"
        };

        // 3. Create request with mandatory encryption tag
        let request_payload = serde_json::json!({
            "method": "make_invoice",
            "params": {
                "amount": amount_msat,
                "description_hash": description_hash,
            }
        });

        // Build tags according to NIP-47
        let extra_tags = vec![
            vec![Value::String("encryption".into()), Value::String(chosen_scheme.into())],
        ];

        // 4. Dispatch NWC via relay
        let resp_json = connection.send_request(transport, &request_payload, Some(extra_tags)).await?;

        if let Some(result) = resp_json.get("result") {
            if let Some(invoice) = result.get("invoice").and_then(|i| i.as_str()) {
                return Ok(invoice.to_string());
            }
        }
        
        worker::console_error!("make_invoice: malformed response: {:?}", resp_json);
        Err(Error::from("Malformed response: missing invoice result"))
    }
}

