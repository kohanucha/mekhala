use crate::common::{InternalConnection, InternalTransport};
use crate::nostr::nip_47::{
    EncryptionMethod, NwcClient, NwcMethod, NwcResponse, NwcUri,
};
use crate::nostr::Event;
use serde_json::Value;
use worker::*;

pub struct WalletConnector {
    pub client: NwcClient,
}

impl From<crate::nostr::RelayError> for Error {
    fn from(e: crate::nostr::RelayError) -> Self {
        Error::from(e.to_string())
    }
}

impl WalletConnector {
    pub fn new(nwc_uri: &str) -> Result<Self> {
        let uri = NwcUri::from_uri(nwc_uri)?;
        let client = NwcClient::new(uri)?;
        Ok(Self { client })
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
        let mut client = self.negotiate_encryption(transport).await?;

        let (event, request_id) = client.create_request_event(method, params, vec![])?;

        let resp_json = self
            .dispatch(
                transport,
                &mut client,
                &event,
                &request_id,
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
    ) -> Result<NwcClient> {
        let mut client = self.client.clone();

        let info = transport
            .get_wallet_info(&client.wallet_pubkey)
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
            client.encryption_method = EncryptionMethod::Nip44;
        } else {
            client.encryption_method = EncryptionMethod::Nip04;
        };

        Ok(client)
    }

    /// Internal helper to dispatch an NWC request and wait for the response.
    async fn dispatch<T: InternalTransport>(
        &self,
        transport: &T,
        client: &mut NwcClient,
        event: &Event,
        request_id: &str,
    ) -> Result<Value> {
        let internal_connection = InternalConnection::new(transport).await?;

        self.subscribe(
            transport,
            internal_connection.id(),
            request_id,
            &client.my_pubkey,
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

        self.process_response(client, &msg_text, request_id)
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
        client: &NwcClient,
        msg_text: &str,
        request_id: &str,
    ) -> Result<Value> {
        let arr: Vec<serde_json::Value> =
            serde_json::from_str(msg_text).map_err(|e| Error::from(e.to_string()))?;

        if arr.len() >= 3 && arr[0].as_str() == Some("EVENT") {
            let resp_event: Event =
                serde_json::from_value(arr[2].clone()).map_err(|e| Error::from(e.to_string()))?;

            client.parse_response_event(&resp_event, request_id).map_err(|e| Error::from(e.to_string()))
        } else {
            worker::console_error!("NWC dispatch: malformed response from relay: {}", msg_text);
            Err(Error::from("Malformed response from dispatch"))
        }
    }
}
