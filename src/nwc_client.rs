use k256::{PublicKey as K256PublicKey, SecretKey as K256SecretKey};
use k256::schnorr::{SigningKey, signature::hazmat::PrehashSigner};
use aes::Aes256;
use cbc::{Encryptor, Decryptor};
use block_padding::Pkcs7;
use cipher::{BlockEncryptMut, BlockDecryptMut, KeyIvInit};
use base64::{Engine as _, engine::general_purpose};
use serde_json::Value;
use url::Url;
use rand::RngCore;
use sha2::{Sha256, Digest};
use worker::*;
use futures_util::StreamExt;
use async_trait::async_trait;
use crate::domain::{Event};
use crate::protocol::{KIND_NWC_REQUEST, KIND_NWC_RESPONSE};
use crate::platform::Platform;

#[derive(Debug, Clone)]
pub struct NwcConnection {
    pub wallet_pubkey: String,
    pub secret: String,
}

impl NwcConnection {
    pub fn from_uri(uri: &str) -> Result<Self> {
        let url = Url::parse(uri).map_err(|e| Error::from(e.to_string()))?;
        if url.scheme() != "nostr+walletconnect" {
            return Err(Error::from("Invalid scheme"));
        }

        let wallet_pubkey = url.host_str().ok_or_else(|| Error::from("Missing wallet pubkey"))?.to_string();
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();

        let secret = query.get("secret").ok_or_else(|| Error::from("Missing secret"))?.clone();

        Ok(Self {
            wallet_pubkey,
            secret,
        })
    }
}

pub struct NwcSession {
    pub wallet_pubkey: String,
    shared_secret: Vec<u8>,
    signing_key: SigningKey,
    pub my_pubkey: String,
}

impl NwcSession {
    pub fn new(conn: NwcConnection) -> Result<Self> {
        let shared_secret = get_shared_secret(&conn.secret, &conn.wallet_pubkey)?;
        
        let sk_bytes = hex::decode(&conn.secret).map_err(|e| Error::from(e.to_string()))?;
        let sk_bytes_arr: [u8; 32] = sk_bytes.try_into().map_err(|_| Error::from("Invalid secret key length"))?;
        let signing_key = SigningKey::from_bytes(&sk_bytes_arr).map_err(|e| Error::from(e.to_string()))?;
        let my_pubkey = hex::encode(&signing_key.verifying_key().to_bytes());

        Ok(Self {
            wallet_pubkey: conn.wallet_pubkey,
            shared_secret,
            signing_key,
            my_pubkey,
        })
    }

    pub fn encrypt(&self, payload: &Value) -> Result<String> {
        encrypt_nip04(&self.shared_secret, &payload.to_string())
    }

    pub fn decrypt(&self, encrypted: &str) -> Result<String> {
        decrypt_nip04(&self.shared_secret, encrypted)
    }

    pub fn create_event(&self, kind: u64, content: String, tags: Vec<Vec<Value>>) -> Result<Event> {
        let created_at = Platform::now();
        
        let serialized = serde_json::to_string(&(0, &self.my_pubkey, created_at, kind, &tags, &content))
            .map_err(|e| Error::from(e.to_string()))?;

        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        let id_bytes = hasher.finalize();
        let id = hex::encode(id_bytes);
        
        let signature = self.signing_key.sign_prehash(&id_bytes).map_err(|e| Error::from(e.to_string()))?;
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

    /// Deepened protocol orchestration: performs a full NWC request-response cycle.
    pub async fn call<T: Transport>(&self, transport: &mut T, request_payload: &Value) -> Result<Value> {
        // 1. Encrypt and wrap the payload into a Nostr EVENT
        let encrypted_content = self.encrypt(request_payload)?;
        let tags = vec![vec![Value::String("p".into()), Value::String(self.wallet_pubkey.clone())]];
        let event = self.create_event(KIND_NWC_REQUEST, encrypted_content, tags)?;

        // 2. Format Nostr REQ (subscription)
        let sub_id = hex::encode(rand::thread_rng().next_u32().to_be_bytes());
        let req_msg = serde_json::json!(["REQ", sub_id, {
            "kinds": [KIND_NWC_RESPONSE],
            "#p": [self.my_pubkey],
            "#e": [event.id],
            "since": event.created_at - 1
        }]).to_string();

        // 3. Dispatch messages via transport
        transport.send(&req_msg).await?;
        transport.send(&serde_json::json!(["EVENT", event]).to_string()).await?;

        // 4. Wait for matching response
        let timeout_at = Platform::now_ms() + 10000;
        
        while Platform::now_ms() < timeout_at {
            let remaining = timeout_at.saturating_sub(Platform::now_ms());
            if remaining == 0 { break; }

            let msg_text = transport.receive(remaining).await?;
            let arr: Vec<serde_json::Value> = serde_json::from_str(&msg_text).map_err(|e| Error::from(e.to_string()))?;
            
            if arr.len() >= 3 && arr[0] == "EVENT" && arr[1] == sub_id {
                let resp_event: Event = serde_json::from_value(arr[2].clone()).map_err(|e| Error::from(e.to_string()))?;
                let decrypted = self.decrypt(&resp_event.content)?;
                let resp_json: Value = serde_json::from_str(&decrypted).map_err(|e| Error::from(e.to_string()))?;
                
                // Handle NWC Error responses
                if let Some(error) = resp_json.get("error") {
                    return Err(Error::from(format!("NWC Error: {:?}", error)));
                }
                
                return Ok(resp_json);
            }
        }

        Err(Error::from("Timeout waiting for response from wallet"))
    }
}

/// Transport abstracts the communication layer for Nostr messages.
#[async_trait(?Send)]
pub trait Transport {
    async fn send(&self, msg: &str) -> Result<()>;
    async fn receive(&mut self, timeout_ms: u64) -> Result<String>;
}

/// InternalRelayClient is the factory for internal connections.
pub struct InternalRelayClient {
    stub: Stub,
}

impl InternalRelayClient {
    pub fn new(stub: Stub) -> Self {
        Self { stub }
    }

    /// Establishes the internal connection and returns a Transport adapter.
    pub async fn connect(&self, wallet_pubkey: &str) -> Result<InternalRelayTransport> {
        // 1. Check if the target wallet is actually online
        let check_req = Request::new(&format!("http://internal/check/{}", wallet_pubkey), Method::Get)?;
        let mut check_resp = self.stub.fetch_with_request(check_req).await?;
        if check_resp.text().await? != "OK" {
            return Err(Error::from("Wallet not connected"));
        }

        // 2. Open internal WebSocket
        let mut ws_req = Request::new("http://internal/", Method::Get)?;
        ws_req.headers_mut()?.set("Upgrade", "websocket")?;
        ws_req.headers_mut()?.set("Connection", "Upgrade")?;

        let response = self.stub.fetch_with_request(ws_req).await?;
        let ws = response.websocket().ok_or_else(|| Error::from("Failed to upgrade to internal WebSocket"))?;
        
        ws.accept()?;
        
        Ok(InternalRelayTransport { ws })
    }
}

/// Cloudflare-specific adapter for the Transport seam.
pub struct InternalRelayTransport {
    ws: WebSocket,
}

#[async_trait(?Send)]
impl Transport for InternalRelayTransport {
    async fn send(&self, msg: &str) -> Result<()> {
        self.ws.send_with_str(msg)
    }

    async fn receive(&mut self, _timeout_ms: u64) -> Result<String> {
        let mut stream = self.ws.events()?;
        match stream.next().await {
            Some(Ok(WebsocketEvent::Message(msg))) => {
                msg.text().ok_or_else(|| Error::from("Expected text message"))
            }
            Some(Err(e)) => Err(e),
            _ => Err(Error::from("Connection closed")),
        }
    }
}

pub async fn request_invoice(nwc_uri: &str, amount_msat: u64, description_hash: String, stub: Stub) -> Result<String> {
    let conn = NwcConnection::from_uri(nwc_uri)?;
    let session = NwcSession::new(conn)?;
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

fn get_shared_secret(secret_key_hex: &str, public_key_hex: &str) -> Result<Vec<u8>> {
    let secret_key_bytes = hex::decode(secret_key_hex).map_err(|e| Error::from(e.to_string()))?;
    let sk = K256SecretKey::from_slice(&secret_key_bytes).map_err(|e| Error::from(e.to_string()))?;

    let public_key_bytes = hex::decode(public_key_hex).map_err(|e| Error::from(e.to_string()))?;
    let mut full_pk_bytes = [0u8; 33];
    full_pk_bytes[0] = 0x02;
    full_pk_bytes[1..].copy_from_slice(&public_key_bytes);
    
    let pk = K256PublicKey::from_sec1_bytes(&full_pk_bytes).map_err(|e| Error::from(e.to_string()))?;

    let shared = k256::ecdh::diffie_hellman(sk.to_nonzero_scalar(), pk.as_affine());
    Ok(shared.raw_secret_bytes().to_vec())
}

fn encrypt_nip04(shared_secret: &[u8], plaintext: &str) -> Result<String> {
    let mut iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut iv);

    let pt_bytes = plaintext.as_bytes();
    let mut buffer = vec![0u8; pt_bytes.len() + 16];
    buffer[..pt_bytes.len()].copy_from_slice(pt_bytes);

    let pos = pt_bytes.len();
    let ct = Encryptor::<Aes256>::new_from_slices(shared_secret, &iv)
        .map_err(|e| Error::from(e.to_string()))?
        .encrypt_padded_mut::<Pkcs7>(&mut buffer, pos)
        .map_err(|e| Error::from(e.to_string()))?;

    let iv_b64 = general_purpose::STANDARD.encode(iv);
    let ct_b64 = general_purpose::STANDARD.encode(ct);

    Ok(format!("{}?iv={}", ct_b64, iv_b64))
}

fn decrypt_nip04(shared_secret: &[u8], encrypted_content: &str) -> Result<String> {
    let parts: Vec<&str> = encrypted_content.split("?iv=").collect();
    if parts.len() != 2 {
        return Err(Error::from("Invalid NIP-04 format"));
    }

    let mut ct_bytes = general_purpose::STANDARD.decode(parts[0]).map_err(|e| Error::from(e.to_string()))?;
    let iv_bytes = general_purpose::STANDARD.decode(parts[1]).map_err(|e| Error::from(e.to_string()))?;

    let pt = Decryptor::<Aes256>::new_from_slices(shared_secret, &iv_bytes)
        .map_err(|e| Error::from(e.to_string()))?
        .decrypt_padded_mut::<Pkcs7>(&mut ct_bytes)
        .map_err(|e| Error::from(e.to_string()))?;

    String::from_utf8(pt.to_vec()).map_err(|e| Error::from(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nwc_connection_from_uri() {
        let uri = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let conn = NwcConnection::from_uri(uri).unwrap();
        assert_eq!(conn.wallet_pubkey, "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f");
        assert_eq!(conn.secret, "0101010101010101010101010101010101010101010101010101010101010101");
    }

    #[test]
    fn test_nwc_session_roundtrip() {
        let uri = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let conn = NwcConnection::from_uri(uri).unwrap();
        let session = NwcSession::new(conn).unwrap();
        
        let payload = serde_json::json!({"test": "data"});
        let encrypted = session.encrypt(&payload).unwrap();
        let decrypted = session.decrypt(&encrypted).unwrap();
        let decrypted_json: Value = serde_json::from_str(&decrypted).unwrap();
        assert_eq!(payload, decrypted_json);

        let event = session.create_event(KIND_NWC_REQUEST, encrypted, vec![]).unwrap();
        assert_eq!(event.pubkey, session.my_pubkey);
        assert_eq!(event.kind, KIND_NWC_REQUEST);
    }

    struct MockTransport {
        pub responses: Vec<String>,
    }

    #[async_trait(?Send)]
    impl Transport for MockTransport {
        async fn send(&self, _msg: &str) -> Result<()> {
            Ok(())
        }
        async fn receive(&mut self, _timeout_ms: u64) -> Result<String> {
            if self.responses.is_empty() {
                return Err(Error::from("No responses"));
            }
            Ok(self.responses.remove(0))
        }
    }
}
