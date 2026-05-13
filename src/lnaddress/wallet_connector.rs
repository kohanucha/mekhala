use crate::common::{InternalConnection, InternalTransport};
use crate::nostr::nip_47::{
    EncryptionMethod, NwcMethod, NwcRequest, NwcResponse, WalletConnection,
    WalletConnectionDetails, KIND_NWC_REQUEST,
};
use crate::nostr::Event;
use crate::util::now;
use serde_json::Value;
use worker::*;

pub struct WalletConnector {
    pub connection: WalletConnection,
}

impl WalletConnector {
    pub fn new(nwc_uri: &str) -> Result<Self> {
        let details = WalletConnectionDetails::from_uri(nwc_uri)?;
        let connection = WalletConnection::new(details)?;
        Ok(Self { connection })
    }

    pub async fn make_invoice(
        &self,
        transport: &impl InternalTransport,
        amount_msat: u64,
        description_hash: String,
    ) -> Result<String> {
        let params = serde_json::json!({
            "amount": amount_msat,
            "description_hash": description_hash,
        });

        let result = self
            .execute(transport, NwcMethod::MakeInvoice, params)
            .await?;

        result
            .get("invoice")
            .and_then(|i| i.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| Error::from("Missing invoice in response"))
    }

    /// Unified executor for any NWC method.
    /// Handles negotiation, serialization, dispatch, and error mapping.
    async fn execute<T: InternalTransport>(
        &self,
        transport: &T,
        method: NwcMethod,
        params: Value,
    ) -> Result<Value> {
        let mut wallet_connection = self.negotiate_encryption(transport).await?;

        let request_payload = serde_json::to_value(NwcRequest { method, params })
            .map_err(|e| Error::from(e.to_string()))?;

        let extra_tags = vec![vec![
            Value::String("encryption".into()),
            Value::String(wallet_connection.encryption_method.to_protocol_string()),
        ]];

        let resp_json = self
            .dispatch(
                transport,
                &mut wallet_connection,
                &request_payload,
                Some(extra_tags),
            )
            .await?;

        let response: NwcResponse =
            serde_json::from_value(resp_json).map_err(|e| Error::from(e.to_string()))?;

        if let Some(error) = response.error {
            return Err(Error::from(format!(
                "NWC Error ({}): {}",
                error.code, error.message
            )));
        }

        response
            .result
            .ok_or_else(|| Error::from("NWC response missing result and error"))
    }

    /// Internal helper to negotiate encryption with the wallet.
    async fn negotiate_encryption(
        &self,
        transport: &impl InternalTransport,
    ) -> Result<WalletConnection> {
        let mut wallet_connection = self.connection.clone();

        let info = transport
            .get_wallet_info(&wallet_connection.wallet_pubkey)
            .await
            .ok_or_else(|| Error::from("Wallet not connected"))?;

        if !info.online {
            return Err(Error::from("Wallet not connected"));
        }

        if !info.ready {
            return Err(Error::from("Wallet service info not ready"));
        }

        let encryption_algorithms = info.encryption_algorithms;
        if encryption_algorithms.contains(&"nip44_v2".to_string()) {
            wallet_connection.encryption_method = EncryptionMethod::Nip44;
        } else {
            wallet_connection.encryption_method = EncryptionMethod::Nip04;
        };

        Ok(wallet_connection)
    }
    /// Internal helper to dispatch an NWC request and wait for the response.
    async fn dispatch<T: InternalTransport>(
        &self,
        transport: &T,
        wallet_connection: &mut WalletConnection,
        request_payload: &Value,
        extra_tags: Option<Vec<Vec<Value>>>,
    ) -> Result<Value> {
        let internal_connection = InternalConnection::new(transport).await?;

        let (event, event_id) =
            self.prepare_request_event(wallet_connection, request_payload, extra_tags)?;

        self.subscribe(
            transport,
            internal_connection.id(),
            &event_id,
            &wallet_connection.my_pubkey,
        )
        .await?;

        let msg_text = match internal_connection
            .send_and_receive(serde_json::json!(["EVENT", event]).to_string())
            .await
        {
            Ok(msg) => msg,
            Err(e) => {
                self.cleanup(transport, internal_connection).await;
                return Err(e);
            }
        };

        self.cleanup(transport, internal_connection).await;

        self.process_response(wallet_connection, &msg_text, &event_id)
    }

    fn prepare_request_event(
        &self,
        wallet_connection: &WalletConnection,
        request_payload: &Value,
        extra_tags: Option<Vec<Vec<Value>>>,
    ) -> Result<(Event, String)> {
        let mut tags = vec![
            vec![
                Value::String("p".into()),
                Value::String(wallet_connection.wallet_pubkey.clone()),
            ],
            vec![
                Value::String("expiration".into()),
                Value::String((now() + 60).to_string()),
            ],
        ];
        if let Some(extra) = extra_tags {
            tags.extend(extra);
        }

        let encrypted_content = wallet_connection.encrypt(request_payload)?;
        let event = wallet_connection.create_event(KIND_NWC_REQUEST, encrypted_content, tags)?;
        let event_id = event.id.clone();
        Ok((event, event_id))
    }

    async fn subscribe<T: InternalTransport>(
        &self,
        transport: &T,
        conn_id: u32,
        event_id: &str,
        my_pubkey: &str,
    ) -> Result<()> {
        let req_json = serde_json::json!([
            "REQ",
            "bridge_sub",
            { "#e": [event_id], "#p": [my_pubkey] }
        ])
        .to_string();

        let (tx, _rx) = futures::channel::oneshot::channel();
        transport.send_message(conn_id, req_json, tx).await
    }

    async fn cleanup<T: InternalTransport>(
        &self,
        transport: &T,
        internal_connection: InternalConnection<'_, T>,
    ) {
        let close_envelope = serde_json::json!(["CLOSE", "bridge_sub"]).to_string();
        let (tx, _rx) = futures::channel::oneshot::channel();
        let _ = transport
            .send_message(internal_connection.id(), close_envelope, tx)
            .await;
        let _ = internal_connection.close().await;
    }

    fn process_response(
        &self,
        wallet_connection: &WalletConnection,
        msg_text: &str,
        event_id: &str,
    ) -> Result<Value> {
        let arr: Vec<serde_json::Value> =
            serde_json::from_str(msg_text).map_err(|e| Error::from(e.to_string()))?;

        if arr.len() >= 3 && arr[0].as_str() == Some("EVENT") {
            let resp_event: Event =
                serde_json::from_value(arr[2].clone()).map_err(|e| Error::from(e.to_string()))?;

            resp_event
                .verify(now())
                .map_err(|e| Error::from(e.to_string()))?;

            if resp_event.pubkey != wallet_connection.wallet_pubkey {
                worker::console_error!(
                    "NWC dispatch: pubkey mismatch: expected={}, got={}",
                    wallet_connection.wallet_pubkey,
                    resp_event.pubkey
                );
                return Err(Error::from("Response pubkey mismatch"));
            }

            let has_e_tag = resp_event.tags.iter().any(|t| {
                t.len() >= 2 && t[0].as_str() == Some("e") && t[1].as_str() == Some(event_id)
            });

            if !has_e_tag {
                worker::console_error!(
                    "NWC dispatch: response missing 'e' tag for request {}",
                    event_id
                );
                return Err(Error::from("Response missing 'e' tag for request"));
            }

            let decrypted = wallet_connection.decrypt(&resp_event.content)?;
            let resp_json: Value =
                serde_json::from_str(&decrypted).map_err(|e| Error::from(e.to_string()))?;

            return Ok(resp_json);
        }

        worker::console_error!("NWC dispatch: malformed response from relay: {}", msg_text);
        Err(Error::from("Malformed response from dispatch"))
    }
}
