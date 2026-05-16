use serde_json::Value;
use sha2::{Sha256, Digest};
use url::Url;
use crate::common::NwcTransport;
use crate::common::NwcError;
use crate::lnaddress::wallet_connector::NwcSession;

pub struct LnAddressGateway {
    username: String,
}

impl LnAddressGateway {
    pub fn new(username: &str) -> Self {
        Self {
            username: username.to_string(),
        }
    }

    pub fn pay_request_info(&self, request_url: &Url) -> Value {
        let callback_url = self.build_callback_url(request_url);
        
        serde_json::json!({
            "callback": callback_url,
            "maxSendable": 100000000,
            "minSendable": 1000,
            "metadata": self.generate_metadata(),
            "tag": "payRequest"
        })
    }

    pub async fn create_invoice(
        &self,
        transport: &impl NwcTransport,
        nwc_uri: &str,
        amount_msat: u64,
    ) -> Result<String, NwcError> {
        let description_hash = self.get_description_hash();
        let session = NwcSession::new(transport, nwc_uri)?;
        session.make_invoice(amount_msat, description_hash).await
    }

    fn generate_metadata(&self) -> String {
        format!("[[\"text/plain\",\"Payment to {}\"]]", self.username)
    }

    fn get_description_hash(&self) -> String {
        let metadata = self.generate_metadata();
        let mut hasher = Sha256::new();
        hasher.update(metadata.as_bytes());
        let hash = hasher.finalize();
        hex::encode(hash)
    }

    fn build_callback_url(&self, request_url: &Url) -> String {
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
            protocol, host, port, self.username
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_gateway() {
        let gateway = LnAddressGateway::new("alice");
        assert_eq!(gateway.username, "alice");
    }

    #[test]
    fn test_build_callback_url_local() {
        let gateway = LnAddressGateway::new("alice");
        let url = Url::parse("http://localhost:8787/.well-known/lnurlp/alice").unwrap();
        let callback = gateway.build_callback_url(&url);
        assert_eq!(callback, "http://localhost:8787/lnaddress/alice/callback");
    }

    #[test]
    fn test_build_callback_url_remote() {
        let gateway = LnAddressGateway::new("bob");
        let url = Url::parse("https://relay.com/.well-known/lnurlp/bob").unwrap();
        let callback = gateway.build_callback_url(&url);
        assert_eq!(callback, "https://relay.com/lnaddress/bob/callback");
    }

    #[test]
    fn test_build_callback_url_no_port() {
        let gateway = LnAddressGateway::new("charlie");
        let url = Url::parse("https://relay.com/.well-known/lnurlp/charlie").unwrap();
        let callback = gateway.build_callback_url(&url);
        assert_eq!(callback, "https://relay.com/lnaddress/charlie/callback");
    }

    #[test]
    fn test_generate_metadata_format() {
        let gateway = LnAddressGateway::new("alice");
        let metadata = gateway.generate_metadata();
        assert_eq!(metadata, "[[\"text/plain\",\"Payment to alice\"]]");
    }

    #[test]
    fn test_description_hash_length() {
        let gateway = LnAddressGateway::new("testuser");
        let hash = gateway.get_description_hash();
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_description_hash_deterministic() {
        let gateway = LnAddressGateway::new("testuser");
        let hash1 = gateway.get_description_hash();
        let hash2 = gateway.get_description_hash();
        assert_eq!(hash1, hash2);
    }
}