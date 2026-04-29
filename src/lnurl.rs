use worker::*;
use sha2::{Sha256, Digest};
use crate::utils;
use crate::nwc_client;

pub async fn handle_lnurlp(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    match handle_lnurlp_inner(req, ctx).await {
        Ok(resp) => Ok(resp),
        Err(e) => lnurl_error(&e.to_string()),
    }
}

fn lnurl_error(reason: &str) -> Result<Response> {
    let error_body = serde_json::json!({ "status": "ERROR", "reason": reason });
    utils::create_cors_response(Response::from_json(&error_body)?.with_status(200))
}

async fn handle_lnurlp_inner(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let username = ctx.param("username").ok_or_else(|| Error::from("Missing username"))?;
    let kv = ctx.env.kv("MEKHALA_NWC_KV")?;
    
    if kv.get(username).text().await?.is_none() {
        return Err(Error::from("User not found"));
    }

    let url = req.url()?;
    let is_local = url.host_str() == Some("localhost") || url.host_str() == Some("127.0.0.1");
    let host = url.host_str().unwrap_or_default();
    let port = url.port().map(|p| format!(":{}", p)).unwrap_or_default();
    let protocol = if is_local { "http" } else { "https" };
    
    let metadata = format!("[[\"text/plain\",\"Payment to {}\"]]", username);
    
    let callback = format!("{}://{}{}/lnurlp/{}/callback", protocol, host, port, username);
    
    let info = serde_json::json!({
        "callback": callback,
        "maxSendable": 100000000,
        "minSendable": 1000,
        "metadata": metadata,
        "tag": "payRequest"
    });

    utils::create_cors_response(Response::from_json(&info)?)
}

pub async fn handle_lnurlp_callback(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    match handle_lnurlp_callback_inner(req, ctx).await {
        Ok(resp) => Ok(resp),
        Err(e) => lnurl_error(&e.to_string()),
    }
}

async fn handle_lnurlp_callback_inner(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let username = ctx.param("username").ok_or_else(|| Error::from("Missing username"))?;
    let kv = ctx.env.kv("MEKHALA_NWC_KV")?;
    
    let nwc_uri = match kv.get(username).text().await? {
        Some(uri) => uri,
        None => {
            return Err(Error::from("User not found"));
        }
    };
    
    let url = req.url()?;
    let mut query = url.query_pairs();
    let amount_msat = query
        .find(|(k, _)| k == "amount")
        .and_then(|(_, v)| v.parse::<u64>().ok())
        .ok_or_else(|| Error::from("Missing amount"))?;

    // Reconstruct metadata and hash it to create the description_hash
    let metadata = format!("[[\"text/plain\",\"Payment to {}\"]]", username);
    let mut hasher = Sha256::new();
    hasher.update(metadata.as_bytes());
    let hash = hasher.finalize();
    let description_hash = hex::encode(hash);

    let region = ctx.var("WALLET_REGION").map(|v| v.to_string()).ok();
    let stub = utils::get_durable_stub(&ctx.env, region)?;

    let pr = nwc_client::request_invoice(&nwc_uri, amount_msat, description_hash, stub).await?;
    
    let resp = serde_json::json!({
        "pr": pr,
        "routes": []
    });
    utils::create_cors_response(Response::from_json(&resp)?)
}
