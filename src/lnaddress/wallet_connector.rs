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
#[path = "wallet_connector_test.rs"]
mod wallet_connector_test;