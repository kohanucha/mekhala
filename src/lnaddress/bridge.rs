use worker::*;
use crate::cloudflare::get_durable_stub;
use crate::nostr::request_invoice;
use crate::lnaddress::lnaddress::LNAddress;

pub struct Bridge;

impl Bridge {
    pub async fn request_invoice(ctx: &RouteContext<()>, username: &str, amount_msat: u64) -> Result<String> {
        let kv = ctx.env.kv("MEKHALA_NWC_KV")?;
        let nwc_uri = kv.get(username).text().await?
            .ok_or_else(|| Error::from("User not found"))?;

        let ln_address = LNAddress::new(username);
        let description_hash = ln_address.get_description_hash();

        let region = ctx.var("WALLET_REGION").map(|v| v.to_string()).ok();
        let stub = get_durable_stub(&ctx.env, region)?;

        request_invoice(&nwc_uri, amount_msat, description_hash, stub).await
    }

    pub async fn user_exists(ctx: &RouteContext<()>, username: &str) -> Result<bool> {
        let kv = ctx.env.kv("MEKHALA_NWC_KV")?;
        Ok(kv.get(username).text().await?.is_some())
    }
}