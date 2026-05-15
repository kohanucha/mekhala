use crate::nostr::Event;
use crate::util::now;
use k256::schnorr::{signature::hazmat::PrehashSigner, SigningKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;
use crate::nostr::{RelayError, Result};

pub const KIND_NWC_REQUEST: u64 = 23194;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NwcMethod {
    MakeInvoice
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NwcRequest {
    pub method: NwcMethod,
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NwcResponse {
    pub result: Option<Value>,
    pub error: Option<NwcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NwcError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionMethod {
    Nip04,
    Nip44,
}

impl EncryptionMethod {
    pub fn to_protocol_string(&self) -> String {
        match self {
            EncryptionMethod::Nip04 => "nip04".to_string(),
            EncryptionMethod::Nip44 => "nip44_v2".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NwcUri {
    pub wallet_pubkey: String,
    pub secret: String,
}

impl NwcUri {
    pub fn from_uri(uri: &str) -> Result<Self> {
        let url = Url::parse(uri).map_err(|e| RelayError::UrlError(e.to_string()))?;
        if url.scheme() != "nostr+walletconnect" {
            return Err(RelayError::Generic("Invalid scheme".into()));
        }

        let wallet_pubkey = url
            .host_str()
            .ok_or_else(|| RelayError::Generic("Missing wallet pubkey".into()))?
            .to_string();
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();

        let secret = query
            .get("secret")
            .ok_or_else(|| RelayError::Generic("Missing secret".into()))?
            .clone();

        Ok(Self {
            wallet_pubkey,
            secret,
        })
    }
}

#[derive(Clone)]
pub struct NwcClient {
    pub wallet_pubkey: String,
    shared_secret: Vec<u8>,
    signing_key: SigningKey,
    pub my_pubkey: String,
    pub encryption_method: EncryptionMethod,
}

impl NwcClient {
    pub fn new(uri: NwcUri) -> Result<Self> {
        let shared_secret = crate::nostr::get_shared_secret(&uri.secret, &uri.wallet_pubkey)?;

        let sk_bytes = hex::decode(&uri.secret).map_err(|e| RelayError::MalformedHex(e.to_string()))?;
        let sk_bytes_arr: [u8; 32] = sk_bytes
            .try_into()
            .map_err(|_| RelayError::Generic("Invalid secret key length".into()))?;
        let signing_key =
            SigningKey::from_bytes(&sk_bytes_arr).map_err(|e| RelayError::CryptoError(e.to_string()))?;
        let my_pubkey = hex::encode(&signing_key.verifying_key().to_bytes());

        Ok(Self {
            wallet_pubkey: uri.wallet_pubkey,
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

    pub fn create_request_event(
        &self,
        method: NwcMethod,
        params: Value,
        extra_tags: Vec<Vec<Value>>,
    ) -> Result<(Event, String)> {
        let payload = serde_json::to_value(NwcRequest { method, params })
            .map_err(|e| RelayError::SerializationError(e.to_string()))?;

        let mut tags = vec![
            vec![
                Value::String("p".into()),
                Value::String(self.wallet_pubkey.clone()),
            ],
            vec![
                Value::String("expiration".into()),
                Value::String((now() + 60).to_string()),
            ],
        ];
        tags.extend(extra_tags);

        // Always add encryption tag for clarity in the protocol
        tags.push(vec![
            Value::String("encryption".into()),
            Value::String(self.encryption_method.to_protocol_string()),
        ]);

        let encrypted_content = self.encrypt(&payload)?;
        let event = self.create_event(KIND_NWC_REQUEST, encrypted_content, tags)?;
        let event_id = event.id.clone();
        Ok((event, event_id))
    }

    pub fn parse_response_event(&self, event: &Event, request_id: &str) -> Result<Value> {
        event.verify(now())?;

        if event.pubkey != self.wallet_pubkey {
            return Err(RelayError::Generic("Response pubkey mismatch".into()));
        }

        let has_e_tag = event.tags.iter().any(|t| {
            t.len() >= 2 && t[0].as_str() == Some("e") && t[1].as_str() == Some(request_id)
        });

        if !has_e_tag {
            return Err(RelayError::Generic("Response missing 'e' tag for request".into()));
        }

        let decrypted = self.decrypt(&event.content)?;
        let resp_json: Value = serde_json::from_str(&decrypted)?;

        Ok(resp_json)
    }

    pub fn create_event(&self, kind: u64, content: String, tags: Vec<Vec<Value>>) -> Result<Event> {
        let created_at = now();

        let serialized =
            serde_json::to_string(&(0, &self.my_pubkey, created_at, kind, &tags, &content))
                .map_err(|e| RelayError::SerializationError(e.to_string()))?;

        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        let id_bytes = hasher.finalize();
        let id = hex::encode(id_bytes);

        let signature = self
            .signing_key
            .sign_prehash(&id_bytes)
            .map_err(|e| RelayError::CryptoError(e.to_string()))?;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nwc_uri_from_uri() {
        let uri = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let nwc_uri = NwcUri::from_uri(uri).unwrap();
        assert_eq!(
            nwc_uri.wallet_pubkey,
            "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f"
        );
        assert_eq!(
            nwc_uri.secret,
            "0101010101010101010101010101010101010101010101010101010101010101"
        );
    }

    #[test]
    fn test_nwc_client_roundtrip() {
        let uri_str = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let nwc_uri = NwcUri::from_uri(uri_str).unwrap();
        let client = NwcClient::new(nwc_uri).unwrap();

        let payload = serde_json::json!({"test": "data"});
        let encrypted = client.encrypt(&payload).unwrap();
        let decrypted = client.decrypt(&encrypted).unwrap();
        let decrypted_json: Value = serde_json::from_str(&decrypted).unwrap();
        assert_eq!(payload, decrypted_json);

        let (event, request_id) = client
            .create_request_event(NwcMethod::MakeInvoice, payload.clone(), vec![])
            .unwrap();
        assert_eq!(event.pubkey, client.my_pubkey);
        assert_eq!(event.kind, KIND_NWC_REQUEST);

        // Test parsing back (mocking a response)
        let resp_payload = serde_json::json!({"result": {"invoice": "lnbc1..."}});
        let resp_encrypted = client.encrypt(&resp_payload).unwrap();
        let resp_event = Event {
            id: "resp_id".into(),
            pubkey: client.wallet_pubkey.clone(),
            created_at: now(),
            kind: 23195,
            tags: vec![
                vec![Value::String("e".into()), Value::String(request_id.clone())],
                vec![Value::String("p".into()), Value::String(client.my_pubkey.clone())],
            ],
            content: resp_encrypted,
            sig: "sig".into(), // verify(now()) will fail in test unless we sign it properly, but we'll bypass verification for this unit test if needed or just sign it.
        };
        
        // Actually we need to sign it to pass verify(now())
        let wallet_sk_bytes = hex::decode("0101010101010101010101010101010101010101010101010101010101010101").unwrap();
        let wallet_sk_arr: [u8; 32] = wallet_sk_bytes.try_into().unwrap();
        let wallet_sk = SigningKey::from_bytes(&wallet_sk_arr).unwrap();
        
        let serialized_resp = serde_json::to_string(&(0, &client.wallet_pubkey, resp_event.created_at, resp_event.kind, &resp_event.tags, &resp_event.content)).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(serialized_resp.as_bytes());
        let resp_id_bytes = hasher.finalize();
        let resp_id = hex::encode(resp_id_bytes);
        let resp_sig = hex::encode(wallet_sk.sign_prehash(&resp_id_bytes).unwrap().to_bytes());
        
        let signed_resp_event = Event {
            id: resp_id,
            sig: resp_sig,
            ..resp_event
        };

        let parsed_resp = client.parse_response_event(&signed_resp_event, &request_id).unwrap();
        assert_eq!(parsed_resp, resp_payload);
    }

    #[test]
    fn test_nwc_client_nip44_roundtrip() {
        let uri_str = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let nwc_uri = NwcUri::from_uri(uri_str).unwrap();
        let mut client = NwcClient::new(nwc_uri).unwrap();
        client.encryption_method = EncryptionMethod::Nip44;

        let payload = serde_json::json!({"test": "nip44 data"});
        let encrypted = client.encrypt(&payload).unwrap();
        let decrypted = client.decrypt(&encrypted).unwrap();
        let decrypted_json: Value = serde_json::from_str(&decrypted).unwrap();
        assert_eq!(payload, decrypted_json);
    }

    #[test]
    #[should_panic(expected = "Invalid scheme")]
    fn test_uri_invalid_scheme() {
        let uri = "http://invalid.example.com?secret=0101010101010101010101010101010101010101010101010101010101010101";
        let _ = NwcUri::from_uri(uri).unwrap();
    }

    #[test]
    fn test_uri_missing_pubkey_returns_error() {
        let uri = "nostr+walletconnect://?secret=0101010101010101010101010101010101010101010101010101010101010101";
        let result = NwcUri::from_uri(uri);
        assert!(result.is_err());
    }

    #[test]
    fn test_uri_missing_secret_returns_error() {
        let uri = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f";
        let result = NwcUri::from_uri(uri);
        assert!(result.is_err());
    }

    #[test]
    fn test_client_encrypt_deterministic() {
        let uri_str = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let nwc_uri = NwcUri::from_uri(uri_str).unwrap();
        let client = NwcClient::new(nwc_uri).unwrap();

        let payload = serde_json::json!({"same": "data"});
        let encrypted1 = client.encrypt(&payload).unwrap();
        let encrypted2 = client.encrypt(&payload).unwrap();
        assert_ne!(encrypted1, encrypted2);
    }

    #[test]
    fn test_client_created_has_required_fields() {
        let uri_str = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let nwc_uri = NwcUri::from_uri(uri_str).unwrap();
        let client = NwcClient::new(nwc_uri).unwrap();

        assert!(!client.my_pubkey.is_empty());
    }

    #[test]
    fn test_kind_constants() {
        assert_eq!(KIND_NWC_REQUEST, 23194);
    }
}
