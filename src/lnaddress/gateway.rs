use serde_json::Value;
use sha2::{Sha256, Digest};
use url::Url;
use crate::common::NwcTransport;
use crate::common::NwcError;
use crate::lnaddress::wallet_connector::NwcSession;

pub fn pay_request_info(username: &str, request_url: &Url) -> Value {
    let callback_url = build_callback_url(username, request_url);

    serde_json::json!({
        "callback": callback_url,
        "maxSendable": 100000000,
        "minSendable": 1000,
        "metadata": generate_metadata(username),
        "tag": "payRequest"
    })
}

pub async fn create_invoice(
    transport: &impl NwcTransport,
    nwc_uri: &str,
    username: &str,
    amount_msat: u64,
) -> Result<String, NwcError> {
    let description_hash = get_description_hash(username);
    let session = NwcSession::new(transport, nwc_uri)?;
    session.make_invoice(amount_msat, description_hash).await
}

fn generate_metadata(username: &str) -> String {
    format!("[[\"text/plain\",\"Payment to {}\"]]", username)
}

fn get_description_hash(username: &str) -> String {
    let metadata = generate_metadata(username);
    let mut hasher = Sha256::new();
    hasher.update(metadata.as_bytes());
    let hash = hasher.finalize();
    hex::encode(hash)
}

fn build_callback_url(username: &str, request_url: &Url) -> String {
    let is_local = request_url.host_str() == Some("localhost")
        || request_url.host_str() == Some("127.0.0.1");
    let host = request_url.host_str().unwrap_or_default();
    let port = request_url
        .port()
        .map(|p| format!(":{}", p))
        .unwrap_or_default();
    let protocol = if is_local { "http" } else { "https" };

    format!(
        "{}://{}{}/lnaddress/{}/callback",
        protocol, host, port, username
    )
}

#[cfg(test)]
#[path = "gateway_test.rs"]
mod gateway_test;