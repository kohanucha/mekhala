use crate::common::NwcTransport;
use crate::nostr::nip_47::{
    EncryptionMethod, NwcClient, NwcMethod, NwcResponse, NwcUri,
};
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
        transport: &impl NwcTransport,
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
    async fn execute<T: NwcTransport>(
        &self,
        transport: &T,
        method: NwcMethod,
        params: Value,
    ) -> Result<Value> {
        let client = self.negotiate_encryption(transport).await?;

        let (event, request_id) = client.create_request_event(method, params, vec![])?;

        let resp_event = transport.execute_nwc_rpc(event).await?;

        let resp_json = client
            .parse_response_event(&resp_event, &request_id)
            .map_err(|e| Error::from(e.to_string()))?;

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
        transport: &impl NwcTransport,
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
}
