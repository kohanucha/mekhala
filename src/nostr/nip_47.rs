use crate::nostr::Event;
use crate::nostr::Tag;
use k256::schnorr::{signature::hazmat::PrehashSigner, SigningKey};
use k256::{PublicKey as K256PublicKey, SecretKey as K256SecretKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;
use crate::nostr::{RelayError, Result, Limits};

pub const KIND_NWC_REQUEST: u64 = 23194;

fn get_shared_secret(secret_key_hex: &str, public_key_hex: &str) -> Result<Vec<u8>> {
    let secret_key_bytes = hex::decode(secret_key_hex).map_err(|e| RelayError::MalformedHex(e.to_string()))?;
    let sk =
        K256SecretKey::from_slice(&secret_key_bytes).map_err(|e| RelayError::CryptoError(e.to_string()))?;

    let public_key_bytes = hex::decode(public_key_hex).map_err(|e| RelayError::MalformedHex(e.to_string()))?;
    let mut full_pk_bytes = [0u8; 33];
    full_pk_bytes[0] = 0x02;
    full_pk_bytes[1..].copy_from_slice(&public_key_bytes);

    let pk =
        K256PublicKey::from_sec1_bytes(&full_pk_bytes).map_err(|e| RelayError::CryptoError(e.to_string()))?;

    let shared = k256::ecdh::diffie_hellman(sk.to_nonzero_scalar(), pk.as_affine());
    Ok(shared.raw_secret_bytes().to_vec())
}

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
    pub fn to_protocol_string(self) -> String {
        match self {
            EncryptionMethod::Nip04 => "nip04".to_string(),
            EncryptionMethod::Nip44 => "nip44_v2".to_string(),
        }
    }

    pub fn from_protocol_str(s: &str) -> Option<EncryptionMethod> {
        match s {
            "nip04" => Some(EncryptionMethod::Nip04),
            "nip44_v2" => Some(EncryptionMethod::Nip44),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletInfo {
    pub encryption_algorithms: Vec<EncryptionMethod>,
}

pub fn parse_wallet_info(event: &crate::nostr::Event) -> WalletInfo {
    let mut encryption = Vec::new();
    let mut has_encryption_tag = false;

    for tag in &event.tags {
        if let Some(schemes) = tag.encryption_scheme() {
            has_encryption_tag = true;
            for scheme in schemes.split_whitespace() {
                if let Some(method) = EncryptionMethod::from_protocol_str(scheme) {
                    if !encryption.contains(&method) {
                        encryption.push(method);
                    }
                }
            }
        }
    }

    if !has_encryption_tag {
        encryption.push(EncryptionMethod::Nip04);
    }

    WalletInfo { encryption_algorithms: encryption }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NwcUriError {
    InvalidUrl(String),
    InvalidScheme,
    MissingPubkey,
    MissingSecret,
}

impl std::fmt::Display for NwcUriError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUrl(msg) => write!(f, "error: url failure: {}", msg),
            Self::InvalidScheme => write!(f, "error: Invalid scheme"),
            Self::MissingPubkey => write!(f, "error: Missing wallet pubkey"),
            Self::MissingSecret => write!(f, "error: Missing secret"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NwcUri {
    pub wallet_pubkey: String,
    pub secret: String,
}

impl NwcUri {
    pub fn from_uri(uri: &str) -> std::result::Result<Self, NwcUriError> {
        let url = Url::parse(uri).map_err(|e| NwcUriError::InvalidUrl(e.to_string()))?;
        if url.scheme() != "nostr+walletconnect" {
            return Err(NwcUriError::InvalidScheme);
        }

        let wallet_pubkey = url
            .host_str()
            .ok_or(NwcUriError::MissingPubkey)?
            .to_string();
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();

        let secret = query
            .get("secret")
            .ok_or(NwcUriError::MissingSecret)?
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
    clock: fn() -> u64,
}

impl NwcClient {
    pub fn new(uri: NwcUri) -> Result<Self> {
        let shared_secret = get_shared_secret(&uri.secret, &uri.wallet_pubkey)?;

        let sk_bytes = hex::decode(&uri.secret).map_err(|e| RelayError::MalformedHex(e.to_string()))?;
        let sk_bytes_arr: [u8; 32] = sk_bytes
            .try_into()
            .map_err(|_| RelayError::Generic("Invalid secret key length".into()))?;
        let signing_key =
            SigningKey::from_bytes(&sk_bytes_arr).map_err(|e| RelayError::CryptoError(e.to_string()))?;
        let my_pubkey = hex::encode(signing_key.verifying_key().to_bytes());

        Ok(Self {
            wallet_pubkey: uri.wallet_pubkey,
            shared_secret,
            signing_key,
            my_pubkey,
            encryption_method: EncryptionMethod::Nip04,
            clock: crate::util::now,
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
        extra_tags: Vec<Tag>,
    ) -> Result<(Event, String)> {
        let payload = serde_json::to_value(NwcRequest { method, params })
            .map_err(|e| RelayError::SerializationError(e.to_string()))?;

        let mut tags = vec![
            Tag::p(&self.wallet_pubkey),
            Tag::expiration((self.clock)() + 60),
        ];
        tags.extend(extra_tags);

        tags.push(Tag::encryption(self.encryption_method.to_protocol_string()));

        let encrypted_content = self.encrypt(&payload)?;
        let event = self.create_event(KIND_NWC_REQUEST, encrypted_content, tags)?;
        let event_id = event.id.clone();
        Ok((event, event_id))
    }

    pub fn parse_response_event(&self, event: &Event, request_id: &str) -> Result<Value> {
        event.verify((self.clock)(), &Limits::default())?;

        if event.pubkey != self.wallet_pubkey {
            return Err(RelayError::Generic("Response pubkey mismatch".into()));
        }

        let has_e_tag = event.tags.iter().any(|t| {
            t.event_id().is_some_and(|eid| eid == request_id)
        });

        if !has_e_tag {
            return Err(RelayError::Generic("Response missing 'e' tag for request".into()));
        }

        let decrypted = self.decrypt(&event.content)?;
        let resp_json: Value = serde_json::from_str(&decrypted)?;

        Ok(resp_json)
    }

    pub fn create_event(&self, kind: u64, content: String, tags: Vec<Tag>) -> Result<Event> {
        let created_at = (self.clock)();

        let (id, id_bytes) = Event::compute_id(&self.my_pubkey, created_at, kind, &tags, &content)?;

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
#[path = "nip_47_test.rs"]
mod nip_47_test;
