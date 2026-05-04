use worker::*;
use crate::nostr::nip_47::{ConnectionDetails, Session, EncryptionMethod};
use crate::cloudflare::info_internal;
use serde_json::Value;

pub struct WalletConnector {
    env: Env,
    nwc_uri: String,
}

impl WalletConnector {
    pub fn new(env: &Env, nwc_uri: &str) -> Self {
        Self {
            env: env.clone(),
            nwc_uri: nwc_uri.to_string(),
        }
    }

    pub async fn make_invoice(&self, amount_msat: u64, description_hash: String) -> Result<String> {
        let conn_details = ConnectionDetails::from_uri(&self.nwc_uri)?;
        let mut session = Session::new(conn_details)?;

        // 1. Check wallet capability via internal /info
        let info = info_internal(&self.env, &session.wallet_pubkey).await?;
        
        let online = info.get("online").and_then(|v| v.as_bool()).unwrap_or(false);
        if !online {
            return Err(Error::from("Wallet not connected"));
        }

        let ready = info.get("ready").and_then(|v| v.as_bool()).unwrap_or(false);
        if !ready {
            return Err(Error::from("Wallet service info not ready"));
        }

        let encryption_caps = info.get("encryption").and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();

        // 2. Negotiate encryption: Prefer NIP-44, fallback to NIP-04
        let chosen_scheme = if encryption_caps.contains(&"nip44_v2") {
            session.encryption_method = EncryptionMethod::Nip44;
            "nip44_v2"
        } else {
            session.encryption_method = EncryptionMethod::Nip04;
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

        // 4. Dispatch NWC via synchronous internal HTTP
        let resp_json = session.dispatch(&self.env, &request_payload, Some(extra_tags)).await?;

        if let Some(result) = resp_json.get("result") {
            if let Some(invoice) = result.get("invoice").and_then(|i| i.as_str()) {
                return Ok(invoice.to_string());
            }
        }
        
        Err(Error::from("Malformed response: missing invoice result"))
    }
}
