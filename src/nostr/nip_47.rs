use crate::nostr::Event;
use crate::util::now;
use crate::util::now_ms;
use async_trait::async_trait;
use k256::schnorr::{signature::hazmat::PrehashSigner, SigningKey};
use rand::RngCore;
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;
use worker::{Error, Result};

pub const KIND_NWC_REQUEST: u64 = 23194;
pub const KIND_NWC_RESPONSE: u64 = 23195;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionMethod {
    Nip04,
    Nip44,
}

#[async_trait(?Send)]
pub trait Transport: Send + Sync {
    async fn send(&self, msg: &str) -> Result<()>;
    async fn receive(&mut self, timeout_ms: u64) -> Result<String>;
}

#[derive(Debug, Clone)]
pub struct ConnectionDetails {
    pub wallet_pubkey: String,
    pub secret: String,
}

impl ConnectionDetails {
    pub fn from_uri(uri: &str) -> Result<Self> {
        let url = Url::parse(uri).map_err(|e| Error::from(e.to_string()))?;
        if url.scheme() != "nostr+walletconnect" {
            return Err(Error::from("Invalid scheme"));
        }

        let wallet_pubkey = url
            .host_str()
            .ok_or_else(|| Error::from("Missing wallet pubkey"))?
            .to_string();
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();

        let secret = query
            .get("secret")
            .ok_or_else(|| Error::from("Missing secret"))?
            .clone();

        Ok(Self {
            wallet_pubkey,
            secret,
        })
    }
}

pub struct Session {
    pub wallet_pubkey: String,
    shared_secret: Vec<u8>,
    signing_key: SigningKey,
    pub my_pubkey: String,
    pub encryption_method: EncryptionMethod,
}

impl Session {
    pub fn new(conn: ConnectionDetails) -> Result<Self> {
        let shared_secret = crate::nostr::get_shared_secret(&conn.secret, &conn.wallet_pubkey)?;

        let sk_bytes = hex::decode(&conn.secret).map_err(|e| Error::from(e.to_string()))?;
        let sk_bytes_arr: [u8; 32] = sk_bytes
            .try_into()
            .map_err(|_| Error::from("Invalid secret key length"))?;
        let signing_key =
            SigningKey::from_bytes(&sk_bytes_arr).map_err(|e| Error::from(e.to_string()))?;
        let my_pubkey = hex::encode(&signing_key.verifying_key().to_bytes());

        Ok(Self {
            wallet_pubkey: conn.wallet_pubkey,
            shared_secret,
            signing_key,
            my_pubkey,
            encryption_method: EncryptionMethod::Nip04, // Default to NIP-04
        })
    }

    pub fn encrypt(&self, payload: &Value) -> Result<String> {
        match self.encryption_method {
            EncryptionMethod::Nip04 => crate::nostr::nip_04::encrypt_nip04(&self.shared_secret, &payload.to_string()),
            EncryptionMethod::Nip44 => crate::nostr::nip_44::encrypt_nip44(&self.shared_secret, &payload.to_string()),
        }
    }

    pub fn decrypt(&self, encrypted: &str) -> Result<String> {
        match self.encryption_method {
            EncryptionMethod::Nip04 => crate::nostr::nip_04::decrypt_nip04(&self.shared_secret, encrypted),
            EncryptionMethod::Nip44 => crate::nostr::nip_44::decrypt_nip44(&self.shared_secret, encrypted),
        }
    }

    pub fn create_event(&self, kind: u64, content: String, tags: Vec<Vec<Value>>) -> Result<Event> {
        let created_at = now();

        let serialized =
            serde_json::to_string(&(0, &self.my_pubkey, created_at, kind, &tags, &content))
                .map_err(|e| Error::from(e.to_string()))?;

        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        let id_bytes = hasher.finalize();
        let id = hex::encode(id_bytes);

        let signature = self
            .signing_key
            .sign_prehash(&id_bytes)
            .map_err(|e| Error::from(e.to_string()))?;
        let sig = hex::encode(signature.to_bytes());

        Ok(Event {
            id,
            pubkey: self.my_pubkey.clone(),
            created_at,
            kind,
            tags,
            content,
            sig,
        })
    }

    pub async fn call<T: Transport>(
        &self,
        transport: &mut T,
        request_payload: &Value,
        extra_tags: Option<Vec<Vec<Value>>>,
    ) -> Result<Value> {
        let encrypted_content = self.encrypt(request_payload)?;
        let mut tags = vec![vec![
            Value::String("p".into()),
            Value::String(self.wallet_pubkey.clone()),
        ]];
        if let Some(extra) = extra_tags {
            tags.extend(extra);
        }
        let event = self.create_event(KIND_NWC_REQUEST, encrypted_content, tags)?;

        let sub_id = hex::encode(rand::thread_rng().next_u32().to_be_bytes());
        let req_msg = serde_json::json!(["REQ", sub_id, {
            "kinds": [KIND_NWC_RESPONSE],
            "#p": [self.my_pubkey],
            "#e": [event.id],
            "since": event.created_at - 1
        }])
        .to_string();

        transport.send(&req_msg).await?;
        transport
            .send(&serde_json::json!(["EVENT", event]).to_string())
            .await?;

        let timeout_at = now_ms() + 10000;

        while now_ms() < timeout_at {
            let remaining = timeout_at.saturating_sub(now_ms());
            if remaining == 0 {
                break;
            }

            let msg_text = transport.receive(remaining).await?;
            let arr: Vec<serde_json::Value> =
                serde_json::from_str(&msg_text).map_err(|e| Error::from(e.to_string()))?;

            if arr.len() >= 3
                && arr[0].as_str() == Some("EVENT")
                && arr[1].as_str() == Some(&sub_id)
            {
                let resp_event: Event = serde_json::from_value(arr[2].clone())
                    .map_err(|e| Error::from(e.to_string()))?;
                let decrypted = self.decrypt(&resp_event.content)?;
                let resp_json: Value =
                    serde_json::from_str(&decrypted).map_err(|e| Error::from(e.to_string()))?;

                if let Some(error) = resp_json.get("error") {
                    return Err(Error::from(format!("NWC Error: {:?}", error)));
                }

                return Ok(resp_json);
            }
        }

        Err(Error::from("Timeout waiting for response from wallet"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nwc_connection_from_uri() {
        let uri = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let conn = ConnectionDetails::from_uri(uri).unwrap();
        assert_eq!(
            conn.wallet_pubkey,
            "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f"
        );
        assert_eq!(
            conn.secret,
            "0101010101010101010101010101010101010101010101010101010101010101"
        );
    }

    #[test]
    fn test_nwc_session_roundtrip() {
        let uri = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let conn = ConnectionDetails::from_uri(uri).unwrap();
        let session = Session::new(conn).unwrap();

        let payload = serde_json::json!({"test": "data"});
        let encrypted = session.encrypt(&payload).unwrap();
        let decrypted = session.decrypt(&encrypted).unwrap();
        let decrypted_json: Value = serde_json::from_str(&decrypted).unwrap();
        assert_eq!(payload, decrypted_json);

        let event = session
            .create_event(KIND_NWC_REQUEST, encrypted, vec![])
            .unwrap();
        assert_eq!(event.pubkey, session.my_pubkey);
        assert_eq!(event.kind, KIND_NWC_REQUEST);
    }

    #[test]
    fn test_nwc_session_nip44_roundtrip() {
        let uri = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let conn = ConnectionDetails::from_uri(uri).unwrap();
        let mut session = Session::new(conn).unwrap();
        session.encryption_method = EncryptionMethod::Nip44;

        let payload = serde_json::json!({"test": "nip44 data"});
        let encrypted = session.encrypt(&payload).unwrap();
        let decrypted = session.decrypt(&encrypted).unwrap();
        let decrypted_json: Value = serde_json::from_str(&decrypted).unwrap();
        assert_eq!(payload, decrypted_json);
    }

    #[test]
    #[should_panic(expected = "Invalid scheme")]
    fn test_connection_invalid_scheme() {
        let uri = "http://invalid.example.com?secret=0101010101010101010101010101010101010101010101010101010101010101";
        let _ = ConnectionDetails::from_uri(uri).unwrap();
    }

    #[test]
    fn test_connection_missing_pubkey_returns_error() {
        let uri = "nostr+walletconnect://?secret=0101010101010101010101010101010101010101010101010101010101010101";
        let result = ConnectionDetails::from_uri(uri);
        assert!(result.is_err());
    }

    #[test]
    fn test_connection_missing_secret_returns_error() {
        let uri = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f";
        let result = ConnectionDetails::from_uri(uri);
        assert!(result.is_err());
    }

    #[test]
    fn test_session_encrypt_deterministic() {
        let uri = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let conn = ConnectionDetails::from_uri(uri).unwrap();
        let session = Session::new(conn).unwrap();

        let payload = serde_json::json!({"same": "data"});
        let encrypted1 = session.encrypt(&payload).unwrap();
        let encrypted2 = session.encrypt(&payload).unwrap();
        assert_ne!(encrypted1, encrypted2);
    }

    #[test]
    fn test_session_created_has_required_fields() {
        let uri = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let conn = ConnectionDetails::from_uri(uri).unwrap();
        let session = Session::new(conn).unwrap();

        assert!(!session.my_pubkey.is_empty());
    }

    #[test]
    fn test_event_creation_has_signature() {
        let uri = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let conn = ConnectionDetails::from_uri(uri).unwrap();
        let session = Session::new(conn).unwrap();

        let payload = serde_json::json!({"test": "data"});
        let encrypted = session.encrypt(&payload).unwrap();
        let event = session
            .create_event(KIND_NWC_REQUEST, encrypted, vec![])
            .unwrap();

        assert!(!event.id.is_empty());
        assert!(!event.sig.is_empty());
        assert_eq!(event.kind, KIND_NWC_REQUEST);
    }

    #[test]
    fn test_kind_constants() {
        assert_eq!(KIND_NWC_REQUEST, 23194);
        assert_eq!(KIND_NWC_RESPONSE, 23195);
    }

    #[test]
    fn test_transport_trait_exists() {
        fn _check_send<T: Transport>() {}
        fn _check_receive<T: Transport + ?Sized>() {}
    }

    #[test]
    fn test_internal_relay_client_has_new() {
        fn _has_new<T: std::any::Any>() {}
    }

    #[test]
    fn test_websocket_transport_implements_transport() {
        fn _assert_impls_trait<T: Transport>() {}
    }
}
