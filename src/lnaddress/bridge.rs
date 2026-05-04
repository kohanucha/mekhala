use worker::*;
use crate::cloudflare::{get_nwc_uri, connect_internal};
use crate::lnaddress::lnaddress::LNAddress;
use crate::nostr::nip_47::{ConnectionDetails, Session};

pub struct Bridge;

impl Bridge {
    pub async fn request_invoice(ctx: &RouteContext<()>, username: &str, amount_msat: u64) -> Result<String> {
        let nwc_uri = get_nwc_uri(&ctx.env, username).await?
            .ok_or_else(|| Error::from("User not found"))?;

        let ln_address = LNAddress::new(username);
        let description_hash = ln_address.get_description_hash();

        request_invoice_internal(&ctx.env, &nwc_uri, amount_msat, description_hash).await
    }

    pub async fn user_exists(ctx: &RouteContext<()>, username: &str) -> Result<bool> {
        Ok(get_nwc_uri(&ctx.env, username).await?.is_some())
    }
}

async fn request_invoice_internal(
    env: &Env,
    nwc_uri: &str,
    amount_msat: u64,
    description_hash: String,
) -> Result<String> {
    let conn = ConnectionDetails::from_uri(nwc_uri)?;
    let session = Session::new(conn)?;
    
    let mut transport = connect_internal(env, &session.wallet_pubkey).await?;

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
