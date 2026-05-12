use worker::*;
use crate::nostr::nip_47::{WalletConnectionDetails, WalletConnection, EncryptionMethod, KIND_NWC_REQUEST};
use crate::nostr::Event;
use crate::util::now;
use crate::common::InternalTransport;
use serde_json::Value;

pub struct WalletConnector {
    pub connection: WalletConnection,
}

impl WalletConnector {
    pub fn new(nwc_uri: &str) -> Result<Self> {
        let details = WalletConnectionDetails::from_uri(nwc_uri)?;
        let connection = WalletConnection::new(details)?;
        Ok(Self { connection })
    }

    pub async fn make_invoice(&self, transport: &impl InternalTransport, amount_msat: u64, description_hash: String) -> Result<String> {
        let mut connection = self.connection.clone();

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
        let resp_json = self.send_request(transport, &mut connection, &request_payload, Some(extra_tags)).await?;

        if let Some(result) = resp_json.get("result") {
            if let Some(invoice) = result.get("invoice").and_then(|i| i.as_str()) {
                return Ok(invoice.to_string());
            }
        }
        
        worker::console_error!("make_invoice: malformed response: {:?}", resp_json);
        Err(Error::from("Malformed response: missing invoice result"))
    }

    async fn send_request(
        &self,
        transport: &impl InternalTransport,
        connection: &mut WalletConnection,
        request_payload: &Value,
        extra_tags: Option<Vec<Vec<Value>>>,
    ) -> Result<Value> {
        // 1. Establish Virtual Connection
        let connection_id = transport.create_connection().await?;

        // 2. Prepare Request Event (Calculate ID early)
        let mut tags = vec![
            vec![Value::String("p".into()), Value::String(connection.wallet_pubkey.clone())],
            vec![Value::String("expiration".into()), Value::String((now() + 60).to_string())],
        ];
        if let Some(extra) = extra_tags {
            tags.extend(extra);
        }

        let encrypted_content = connection.encrypt(request_payload)?;
        let event = connection.create_event(KIND_NWC_REQUEST, encrypted_content, tags)?;
        let event_id = event.id.clone();

        // 3. Subscribe (Listener) - Start listening BEFORE sending the event
        let req_json = serde_json::json!([
            "REQ",
            "bridge_sub",
            { "#e": [event_id.clone()], "#p": [connection.my_pubkey] }
        ]).to_string();
        transport.send_message(connection_id, req_json).await?;

        // 4. Send Event
        let event_envelope = serde_json::json!(["EVENT", event]).to_string();
        transport.send_message(connection_id, event_envelope).await?;

        // 5. Wait for Response
        let msg_text = transport.receive_message(connection_id).await?;

        // Cleanup: Always close connection and subscription
        let close_envelope = serde_json::json!(["CLOSE", "bridge_sub"]).to_string();
        let _ = transport.send_message(connection_id, close_envelope).await;
        let _ = transport.close_connection(connection_id).await;

        // 7. Process Response
        let arr: Vec<serde_json::Value> =
            serde_json::from_str(&msg_text).map_err(|e| Error::from(e.to_string()))?;

        if arr.len() >= 3 && arr[0].as_str() == Some("EVENT") {
            let resp_event: Event = serde_json::from_value(arr[2].clone())
                .map_err(|e| Error::from(e.to_string()))?;
            
            // 1. Protocol Verification (Signature & IDs)
            resp_event.verify(now()).map_err(|e| Error::from(e.to_string()))?;

            if resp_event.pubkey != connection.wallet_pubkey {
                worker::console_error!("NWC dispatch: pubkey mismatch: expected={}, got={}", connection.wallet_pubkey, resp_event.pubkey);
                return Err(Error::from("Response pubkey mismatch"));
            }
            let has_e_tag = resp_event.tags.iter().any(|t| {
                t.len() >= 2 && t[0].as_str() == Some("e") && t[1].as_str() == Some(&event_id)
            });
            if !has_e_tag {
                worker::console_error!("NWC dispatch: response missing 'e' tag for request {}", event_id);
                return Err(Error::from("Response missing 'e' tag for request"));
            }

            // 2. Content Decryption
            let decrypted = connection.decrypt(&resp_event.content)?;
            let resp_json: Value =
                serde_json::from_str(&decrypted).map_err(|e| Error::from(e.to_string()))?;

            if let Some(error) = resp_json.get("error") {
                let code = error.get("code").and_then(|c| c.as_str()).unwrap_or("UNKNOWN");
                let message = error.get("message").and_then(|m| m.as_str()).unwrap_or("No message provided");
                worker::console_error!("NWC dispatch: NWC Error ({}): {}", code, message);
                return Err(Error::from(format!("NWC Error ({}): {}", code, message)));
            }

            return Ok(resp_json);
        }

        worker::console_error!("NWC dispatch: malformed response from relay: {}", msg_text);
        Err(Error::from("Malformed response from dispatch"))
    }
}
