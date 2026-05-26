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
mod tests {
    use super::*;

    #[test]
    fn test_build_callback_url_local() {
        let url = Url::parse("http://localhost:8787/.well-known/lnurlp/alice").unwrap();
        let callback = build_callback_url("alice", &url);
        assert_eq!(callback, "http://localhost:8787/lnaddress/alice/callback");
    }

    #[test]
    fn test_build_callback_url_remote() {
        let url = Url::parse("https://relay.com/.well-known/lnurlp/bob").unwrap();
        let callback = build_callback_url("bob", &url);
        assert_eq!(callback, "https://relay.com/lnaddress/bob/callback");
    }

    #[test]
    fn test_build_callback_url_no_port() {
        let url = Url::parse("https://relay.com/.well-known/lnurlp/charlie").unwrap();
        let callback = build_callback_url("charlie", &url);
        assert_eq!(callback, "https://relay.com/lnaddress/charlie/callback");
    }

    #[test]
    fn test_generate_metadata_format() {
        let metadata = generate_metadata("alice");
        assert_eq!(metadata, "[[\"text/plain\",\"Payment to alice\"]]");
    }

    #[test]
    fn test_description_hash_length() {
        let hash = get_description_hash("testuser");
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_description_hash_deterministic() {
        let hash1 = get_description_hash("testuser");
        let hash2 = get_description_hash("testuser");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_pay_request_info_structure() {
        let url = Url::parse("https://relay.com/.well-known/lnurlp/alice").unwrap();
        let info = pay_request_info("alice", &url);
        assert_eq!(info["tag"], "payRequest");
        assert_eq!(info["callback"], "https://relay.com/lnaddress/alice/callback");
        assert_eq!(info["maxSendable"], 100000000);
        assert_eq!(info["minSendable"], 1000);
        assert_eq!(info["metadata"], "[[\"text/plain\",\"Payment to alice\"]]");
    }

    #[test]
    fn test_pay_request_info_different_username() {
        let url = Url::parse("https://other.com/.well-known/lnurlp/bob").unwrap();
        let info = pay_request_info("bob", &url);
        assert_eq!(info["callback"], "https://other.com/lnaddress/bob/callback");
        assert_eq!(info["metadata"], "[[\"text/plain\",\"Payment to bob\"]]");
    }

    #[test]
    fn test_create_invoice_invalid_uri() {
        let _url = Url::parse("https://relay.com/lnurlp/test").unwrap();
        let result = create_invoice(&MockTransport, "not-a-valid-uri", "test", 1000);
        let err = futures::executor::block_on(result).unwrap_err();
        assert!(matches!(err, NwcError::ProtocolError(_)));
    }

    struct MockTransport;

    #[async_trait::async_trait(?Send)]
    impl NwcTransport for MockTransport {
        async fn get_wallet_info(&self, _pubkey: &str) -> Option<crate::nostr::WalletInfo> {
            None
        }
        async fn execute_nwc_rpc(&self, _request: crate::nostr::Event) -> Result<crate::nostr::Event, NwcError> {
            Err(NwcError::WalletNotFound)
        }
    }
}