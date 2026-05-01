use worker::*;
use sha2::{Sha256, Digest};
use crate::platform::Platform;
use crate::nwc_client;
use serde_json::Value;

/// Represents an LN Address (user@relay.com) and handles LUD-06/LUD-16 protocol logic.
/// This module is "pure" Rust, completely decoupled from Cloudflare Worker I/O.
pub struct LNAddress<'a> {
    pub username: &'a str,
}

impl<'a> LNAddress<'a> {
    pub fn new(username: &'a str) -> Self {
        Self { username }
    }

    /// Centralized metadata generation to ensure consistency between endpoints.
    pub fn generate_metadata(&self) -> String {
        format!("[[\"text/plain\",\"Payment to {}\"]]", self.username)
    }

    /// Generates the payload for the LUD-06/LUD-16 payRequest info endpoint.
    pub fn get_info(&self, callback_url: &str) -> Value {
        serde_json::json!({
            "callback": callback_url,
            "maxSendable": 100000000,
            "minSendable": 1000,
            "metadata": self.generate_metadata(),
            "tag": "payRequest"
        })
    }

    /// Generates the description hash required by the NWC backend.
    pub fn get_description_hash(&self) -> String {
        let metadata = self.generate_metadata();
        let mut hasher = Sha256::new();
        hasher.update(metadata.as_bytes());
        let hash = hasher.finalize();
        hex::encode(hash)
    }
}

// --- HTTP Handlers (I/O Layer) ---

pub async fn handle_lnurlp(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    match handle_lnurlp_inner(req, ctx).await {
        Ok(resp) => Ok(resp),
        Err(e) => lnurl_error(&e.to_string()),
    }
}

fn lnurl_error(reason: &str) -> Result<Response> {
    let error_body = serde_json::json!({ "status": "ERROR", "reason": reason });
    Platform::create_cors_response(Response::from_json(&error_body)?.with_status(200))
}

async fn handle_lnurlp_inner(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let username = ctx.param("username").ok_or_else(|| Error::from("Missing username"))?;
    
    let kv = ctx.env.kv("MEKHALA_NWC_KV")?;
    if kv.get(username).text().await?.is_none() {
        return Err(Error::from("User not found"));
    }

    let ln_address = LNAddress::new(username);

    let url = req.url()?;
    let is_local = url.host_str() == Some("localhost") || url.host_str() == Some("127.0.0.1");
    let host = url.host_str().unwrap_or_default();
    let port = url.port().map(|p| format!(":{}", p)).unwrap_or_default();
    let protocol = if is_local { "http" } else { "https" };
    
    let callback_url = format!("{}://{}{}/lnurlp/{}/callback", protocol, host, port, username);
    let info = ln_address.get_info(&callback_url);

    Platform::create_cors_response(Response::from_json(&info)?)
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
        None => return Err(Error::from("User not found")),
    };
    
    let url = req.url()?;
    let mut query = url.query_pairs();
    let amount_msat = query
        .find(|(k, _)| k == "amount")
        .and_then(|(_, v)| v.parse::<u64>().ok())
        .ok_or_else(|| Error::from("Missing amount"))?;

    let ln_address = LNAddress::new(username);
    let description_hash = ln_address.get_description_hash();

    let region = ctx.var("WALLET_REGION").map(|v| v.to_string()).ok();
    let stub = Platform::get_durable_stub(&ctx.env, region)?;

    let pr = nwc_client::request_invoice(&nwc_uri, amount_msat, description_hash, stub).await?;
    
    let resp = serde_json::json!({
        "pr": pr,
        "routes": []
    });
    Platform::create_cors_response(Response::from_json(&resp)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lnaddress_metadata() {
        let addr = LNAddress::new("testuser");
        assert_eq!(addr.generate_metadata(), "[[\"text/plain\",\"Payment to testuser\"]]");
    }

    #[test]
    fn test_lnaddress_description_hash() {
        let addr = LNAddress::new("testuser");
        let hash = addr.get_description_hash();
        
        let mut expected_hasher = Sha256::new();
        expected_hasher.update(b"[[\"text/plain\",\"Payment to testuser\"]]");
        let expected_hash = hex::encode(expected_hasher.finalize());
        
        assert_eq!(hash, expected_hash);
    }

    #[test]
    fn test_lnaddress_info() {
        let addr = LNAddress::new("testuser");
        let info = addr.get_info("https://callback.url");
        assert_eq!(info["callback"], "https://callback.url");
        assert_eq!(info["maxSendable"], 100000000);
        assert_eq!(info["minSendable"], 1000);
        assert_eq!(info["tag"], "payRequest");
        assert_eq!(info["metadata"], "[[\"text/plain\",\"Payment to testuser\"]]");
    }
}
