use worker::*;
use crate::cloudflare::{get_durable_stub, get_nwc_uri, InternalRelayClient};
use crate::lnaddress::lnaddress::LNAddress;
use crate::nostr::nip_47::{ConnectionDetails, Session};

pub struct Bridge;

impl Bridge {
    pub async fn request_invoice(ctx: &RouteContext<()>, username: &str, amount_msat: u64) -> Result<String> {
        let nwc_uri = get_nwc_uri(&ctx.env, username).await?
            .ok_or_else(|| Error::from("User not found"))?;

        let ln_address = LNAddress::new(username);
        let description_hash = ln_address.get_description_hash();

        let stub = get_durable_stub(&ctx.env)?;

        request_invoice_internal(&nwc_uri, amount_msat, description_hash, stub).await
    }

    pub async fn user_exists(ctx: &RouteContext<()>, username: &str) -> Result<bool> {
        Ok(get_nwc_uri(&ctx.env, username).await?.is_some())
    }
}

async fn request_invoice_internal(
    nwc_uri: &str,
    amount_msat: u64,
    description_hash: String,
    stub: Stub,
) -> Result<String> {
    let conn = ConnectionDetails::from_uri(nwc_uri)?;
    let session = Session::new(conn)?;
    let client = InternalRelayClient::new(stub);

    let mut transport = client.connect(&session.wallet_pubkey).await?;

    let request_json = serde_json::json!({
        "method": "make_invoice",
        "params": {
            "amount": amount_msat,
            "description_hash": description_hash,
        }
    });

    let resp_json = session.call(&mut transport, &request_json).await?;

    if let Some(result) = resp_json.get("result") {
        if let Some(invoice) = result.get("invoice").and_then(|i| i.as_str()) {
            return Ok(invoice.to_string());
        }
    }

    Err(Error::from("Malformed response: missing invoice result"))
}
