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
use crate::relay::{Event, KIND_NWC_REQUEST, KIND_NWC_RESPONSE};

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

pub fn get_shared_secret(secret_key_hex: &str, public_key_hex: &str) -> Result<Vec<u8>> {
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

pub fn encrypt_nip04(shared_secret: &[u8], plaintext: &str) -> Result<String> {
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

pub fn decrypt_nip04(shared_secret: &[u8], encrypted_content: &str) -> Result<String> {
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

pub async fn request_invoice(nwc_uri: &str, amount_msat: u64, description_hash: String, stub: Stub) -> Result<String> {
    let nwc = NwcConnection::from_uri(nwc_uri)?;

    // Check if wallet is connected to the Durable Object
    let check_req = Request::new(&format!("http://internal/check/{}", nwc.wallet_pubkey), Method::Get)?;
    let mut check_resp = stub.fetch_with_request(check_req).await?;
    if check_resp.text().await? != "OK" {
        return Err(Error::from("Wallet not connected"));
    }

    let shared_secret = get_shared_secret(&nwc.secret, &nwc.wallet_pubkey)?;

    let params = serde_json::json!({
        "amount": amount_msat,
        "description_hash": description_hash,
    });
    
    let request_json = serde_json::json!({
        "method": "make_invoice",
        "params": params
    });

    let encrypted_content = encrypt_nip04(&shared_secret, &request_json.to_string())?;

    let sk_bytes = hex::decode(&nwc.secret).map_err(|e| Error::from(e.to_string()))?;
    let sk_bytes_arr: [u8; 32] = sk_bytes.try_into().map_err(|_| Error::from("Invalid secret key length"))?;
    let signing_key = SigningKey::from_bytes(&sk_bytes_arr).map_err(|e| Error::from(e.to_string()))?;
    let my_pubkey = hex::encode(&signing_key.verifying_key().to_bytes());

    let created_at = Date::now().as_millis() / 1000;
    let tags = vec![vec![serde_json::Value::String("p".into()), serde_json::Value::String(nwc.wallet_pubkey.clone())]];
    
    let serialized = serde_json::to_string(&(0, &my_pubkey, created_at, KIND_NWC_REQUEST, &tags, &encrypted_content))
        .map_err(|e| Error::from(e.to_string()))?;

    let mut hasher = Sha256::new();
    hasher.update(serialized.as_bytes());
    let id_bytes = hasher.finalize();
    let id = hex::encode(id_bytes);
    
    let signature = signing_key.sign_prehash(&id_bytes).map_err(|e| Error::from(e.to_string()))?;
    let sig = hex::encode(signature.to_bytes());

    let event = Event {
        id: id.clone(),
        pubkey: my_pubkey.clone(),
        created_at,
        kind: KIND_NWC_REQUEST,
        tags: tags.clone(),
        content: encrypted_content,
        sig,
    };

    // Internal WebSocket logic to local Durable Object
    let mut ws_req = Request::new("http://internal/", Method::Get)?;
    ws_req.headers_mut()?.set("Upgrade", "websocket")?;
    ws_req.headers_mut()?.set("Connection", "Upgrade")?;

    let response = stub.fetch_with_request(ws_req).await?;
    let ws = response.websocket().ok_or_else(|| Error::from("Failed to upgrade to internal WebSocket"))?;
    
    ws.accept()?;

    let sub_id = hex::encode(rand::thread_rng().next_u32().to_be_bytes());
    let req_msg = serde_json::json!(["REQ", sub_id, {
        "kinds": [KIND_NWC_RESPONSE],
        "#p": [my_pubkey],
        "#e": [id],
        "since": created_at - 1
    }]).to_string();

    ws.send_with_str(&req_msg)?;
    ws.send_with_str(&serde_json::json!(["EVENT", event]).to_string())?;

    let mut event_stream = ws.events()?;
    let timeout = Date::now().as_millis() + 10000; // 10s timeout

    while Date::now().as_millis() < timeout {
        if let Some(Ok(WebsocketEvent::Message(msg))) = event_stream.next().await {
            if let Some(text) = msg.text() {
                let arr: Vec<serde_json::Value> = serde_json::from_str(&text).map_err(|e| Error::from(e.to_string()))?;
                if arr.len() >= 3 && arr[0] == "EVENT" && arr[1] == sub_id {
                    let resp_event: Event = serde_json::from_value(arr[2].clone()).map_err(|e| Error::from(e.to_string()))?;
                    let decrypted = decrypt_nip04(&shared_secret, &resp_event.content)?;
                    let resp_json: Value = serde_json::from_str(&decrypted).map_err(|e| Error::from(e.to_string()))?;
                    
                    if let Some(result) = resp_json.get("result") {
                        if let Some(invoice) = result.get("invoice").and_then(|i| i.as_str()) {
                            return Ok(invoice.to_string());
                        }
                    }
                    if let Some(error) = resp_json.get("error") {
                        return Err(Error::from(format!("NWC Error: {:?}", error)));
                    }
                }
            }
        }
    }

    Err(Error::from("Timeout waiting for invoice"))
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
    fn test_nwc_connection_invalid_uri() {
        assert!(NwcConnection::from_uri("invalid://test").is_err());
        assert!(NwcConnection::from_uri("nostr+walletconnect://pubkey").is_err()); // missing secret
    }

    #[test]
    fn test_nip04_roundtrip() {
        let shared_secret = vec![0u8; 32];
        let plaintext = "hello nostr";
        let encrypted = encrypt_nip04(&shared_secret, plaintext).unwrap();
        let decrypted = decrypt_nip04(&shared_secret, &encrypted).unwrap();
        assert_eq!(plaintext, decrypted);
    }

    #[test]
    fn test_get_shared_secret() {
        let sk = "0101010101010101010101010101010101010101010101010101010101010101";
        let pk = "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f";
        let shared = get_shared_secret(sk, pk).unwrap();
        assert_eq!(shared.len(), 32);
        
        // Commutative test (sk1 * pk2 == sk2 * pk1)
        let sk2 = "0202020202020202020202020202020202020202020202020202020202020202";
        let pk2 = "4d4b6cd1361032ca9bd2aeb9d900aa4d45d9ead80ac9423374c451a7254d0766";
        
        let shared1 = get_shared_secret(sk, pk2).unwrap();
        let shared2 = get_shared_secret(sk2, pk).unwrap();
        assert_eq!(shared1, shared2);
    }
}
