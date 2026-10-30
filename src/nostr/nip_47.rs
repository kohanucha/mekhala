use crate::nostr::Event;
use crate::nostr::Tag;
use k256::schnorr::{signature::hazmat::PrehashSigner, SigningKey};
use k256::{PublicKey as K256PublicKey, SecretKey as K256SecretKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;
use crate::nostr::{RelayError, Result, Limits};

/// The only Nostr event kind this relay treats as an NWC request.
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

/// NIP-47 request methods a client may invoke on the wallet service.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NwcMethod {
    /// Create an invoice (`make_invoice`).
    MakeInvoice
}

/// Encrypted JSON payload of an NIP-47 request event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NwcRequest {
    /// Requested wallet-service method.
    pub method: NwcMethod,
    /// Method parameters as raw JSON (per NIP-47 schema).
    pub params: Value,
}

/// Encrypted JSON payload of an NIP-47 response event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NwcResponse {
    /// Result value on success.
    pub result: Option<Value>,
    /// Structured error on failure.
    pub error: Option<NwcError>,
}

/// NIP-47 structured error carried inside a [`NwcResponse::error`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NwcError {
    /// Machine-readable NIP-47 error code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

/// Encryption scheme used for NIP-47 event content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionMethod {
    /// AES-256-CBC per NIP-04 (legacy).
    Nip04,
    /// ChaCha20 + HMAC per NIP-44 v2 (preferred).
    Nip44,
}

impl EncryptionMethod {
    /// NIP-47 protocol string used in the `encryption` tag.
    pub fn to_protocol_string(self) -> String {
        match self {
            EncryptionMethod::Nip04 => "nip04".to_string(),
            EncryptionMethod::Nip44 => "nip44_v2".to_string(),
        }
    }

    /// Parses a NIP-47 protocol string from the `encryption` tag.
    pub fn from_protocol_str(s: &str) -> Option<EncryptionMethod> {
        match s {
            "nip04" => Some(EncryptionMethod::Nip04),
            "nip44_v2" => Some(EncryptionMethod::Nip44),
            _ => None,
        }
    }
}

/// Encryption algorithms a wallet service advertised in its info document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletInfo {
    /// Ordered advertised schemes, deduplicated.
    pub encryption_algorithms: Vec<EncryptionMethod>,
}

/// Extracts the advertised encryption algorithms from a wallet's info
/// document event, falling back to [`EncryptionMethod::Nip04`] when no
/// `encryption` tag is present (older wallets).
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

/// Errors produced while parsing a `nostr+walletconnect://` URI.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NwcUriError {
    /// The URI is not a parseable URL; carries the parse failure text.
    #[error("error: url failure: {0}")]
    InvalidUrl(String),
    /// URI scheme is not `nostr+walletconnect`.
    #[error("error: Invalid scheme")]
    InvalidScheme,
    /// No wallet public key in the host position.
    #[error("error: Missing wallet pubkey")]
    MissingPubkey,
    /// No `secret` query parameter.
    #[error("error: Missing secret")]
    MissingSecret,
}

/// Parsed contents of a NIP-47 connection URI: the wallet's public key and
/// the shared secret entered by the user.
#[derive(Debug, Clone)]
pub struct NwcUri {
    /// Wallet service public key (hex, host part of the URI).
    pub wallet_pubkey: String,
    /// Pairing secret from the `secret` query parameter (hex).
    pub secret: String,
}

impl NwcUri {
    /// Parses a `nostr+walletconnect://<pubkey>?secret=<secret>[&relay=...]`
    /// connection URI, ignoring any other query parameters (e.g. `relay`,
    /// `lud16`).
    ///
    /// # Errors
    ///
    /// Returns [`NwcUriError::InvalidUrl`] if the string is not a URL,
    /// [`NwcUriError::InvalidScheme`] on a non-NWC scheme,
    /// [`NwcUriError::MissingPubkey`] when the host is absent, and
    /// [`NwcUriError::MissingSecret`] without a `secret` parameter.
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

/// Client side of an NIP-47 NWC session: encrypts requests to and verifies
/// responses from the wallet service using the shared secret (NIP-04 or
/// NIP-44), plus its own signing key derived from the URI secret.
///
/// Created from [`NwcUri::from_uri`]; the negotiated [`EncryptionMethod`]
/// defaults to NIP-04 and can be set from the wallet's advertised schemes
/// (see [`parse_wallet_info`]).
#[derive(Clone)]
pub struct NwcClient {
    /// Wallet service public key (hex), used as the `p` tag target.
    pub wallet_pubkey: String,
    shared_secret: Vec<u8>,
    signing_key: SigningKey,
    /// Public key of this session's ephemeral identity (hex).
    pub my_pubkey: String,
    /// Content encryption scheme in use.
    pub encryption_method: EncryptionMethod,
    clock: fn() -> u64,
}

impl NwcClient {
    /// Builds a client from a parsed URI, computing the ECDH shared secret
    /// against the wallet key and binding the signing key to the secret.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError::MalformedHex`] for non-hex inputs,
    /// [`RelayError::Generic`] when the secret is not 32 bytes, and
    /// [`RelayError::CryptoError`] for key agreement failures.
    pub fn new(uri: NwcUri) -> Result<Self> {
        let shared_secret = get_shared_secret(&uri.secret, &uri.wallet_pubkey)?;

        let sk_bytes = hex::decode(&uri.secret).map_err(|e| RelayError::MalformedHex(e.to_string()))?;
        let sk_bytes_arr: [u8; 32] = sk_bytes
            .try_into()
            .map_err(|_| RelayError::Generic("Invalid secret key length".into()))?;
        let signing_key =
            SigningKey::from_slice(&sk_bytes_arr).map_err(|e| RelayError::CryptoError(e.to_string()))?;
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

    /// Encrypts the request payload with the negotiated scheme
    /// (see [`EncryptionMethod`]).
    pub fn encrypt(&self, payload: &Value) -> Result<String> {
        match self.encryption_method {
            EncryptionMethod::Nip04 => crate::nostr::nip_04::encrypt_nip04(&self.shared_secret, &payload.to_string()),
            EncryptionMethod::Nip44 => crate::nostr::nip_44::encrypt_nip44(&self.shared_secret, &payload.to_string()),
        }
    }

    /// Decrypts event content with the negotiated scheme
    /// (see [`EncryptionMethod`]).
    pub fn decrypt(&self, encrypted: &str) -> Result<String> {
        match self.encryption_method {
            EncryptionMethod::Nip04 => crate::nostr::nip_04::decrypt_nip04(&self.shared_secret, encrypted),
            EncryptionMethod::Nip44 => crate::nostr::nip_44::decrypt_nip44(&self.shared_secret, encrypted),
        }
    }

    /// Builds a signed NIP-47 request event for the given method and
    /// parameters, appending the required `p`, `expiration` and `encryption`
    /// tags plus any `extra_tags` supplied by the caller.
    ///
    /// Returns the event and its ID (the ID a matching response must carry
    /// in its `e` tag).
    ///
    /// # Errors
    ///
    /// Returns [`RelayError::SerializationError`] if the payload cannot be
    /// serialized, and propagates [`Self::encrypt`] / [`Self::create_event`]
    /// errors.
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

    /// Verifies and decrypts a wallet's response event for `request_id`:
    /// checks the signature/hash, that the event came from the wallet key,
    /// that an `e` tag references the request, and returns the decrypted
    /// JSON payload body.
    ///
    /// # Errors
    ///
    /// Returns verification errors ([`RelayError::InvalidSignature`],
    /// [`RelayError::TimestampTooFar`], ...), [`RelayError::Generic`] on a
    /// pubkey or `e` tag mismatch, and decryption/serialization errors.
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

    /// Signs and assembles a Nostr event with this client's identity and
    /// the current clock; the content/tags/ kind are supplied by the caller.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError::CryptoError`] if prehash signing fails.
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
