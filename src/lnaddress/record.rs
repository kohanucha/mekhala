//! Storage format for a Lightning Address registration.
//!
//! A stored NWC URI is a bearer credential: anyone who reads it can spend from
//! the user's wallet, so it is never written as plaintext. Each record is
//! sealed as a NIP-44 v2 payload under a key derived from the relay's
//! `LN_ADDRESS_KEY` secret, carries the username it belongs to, and is stored
//! under [`KEY_PREFIX`] with the [`SEALED_PREFIX`] marker.
//!
//! The username is part of the sealed payload rather than the key, and is
//! checked after opening: a record copied to another username's key decrypts
//! fine but is refused, so one user's wallet cannot be attached to another
//! address by moving KV rows around.

use crate::nostr::nip_44::{decrypt_nip44, encrypt_nip44};
use crate::nostr::RelayError;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::sync::atomic::{AtomicBool, Ordering};

/// KV key prefix for sealed registrations: `lnaddr:<username>`.
pub const KEY_PREFIX: &str = "lnaddr:";

/// Prefix of a sealed value. Anything else under a `lnaddr:` key is not a
/// record this version understands.
pub const SEALED_PREFIX: &str = "v1:";

/// HKDF info string for deriving the sealing key from the relay secret.
pub const KEY_INFO: &str = "lnaddr:v1";

/// Format version carried inside the sealed payload.
pub const RECORD_VERSION: u8 = 1;

/// Environment variable holding the current sealing key, as 64 hex characters.
pub const KEY_ENV: &str = "LN_ADDRESS_KEY";

/// Environment variable holding the previous sealing key during rotation.
pub const PREVIOUS_KEY_ENV: &str = "LN_ADDRESS_KEY_PREVIOUS";

/// One Lightning Address registration.
///
/// `Debug` is written by hand so the URI cannot end up in a log line.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LnAddressRecord {
    /// Payload format version; must equal [`RECORD_VERSION`].
    #[serde(rename = "v")]
    version: u8,
    /// Username this record belongs to, checked on open.
    #[serde(rename = "u")]
    pub username: String,
    /// The `nostr+walletconnect://` connection URI.
    #[serde(rename = "nwc")]
    pub nwc_uri: String,
    /// Methods the wallet granted, once a registration has verified them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub methods: Option<Vec<String>>,
}

impl LnAddressRecord {
    /// Builds a version-1 record.
    pub fn new(username: &str, nwc_uri: &str, methods: Option<Vec<String>>) -> Self {
        Self {
            version: RECORD_VERSION,
            username: username.to_owned(),
            nwc_uri: nwc_uri.to_owned(),
            methods,
        }
    }
}

impl std::fmt::Debug for LnAddressRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LnAddressRecord")
            .field("version", &self.version)
            .field("username", &self.username)
            .field("nwc_uri", &"<redacted>")
            .field("methods", &self.methods)
            .finish()
    }
}

/// Storage key for one username.
pub fn key_for(username: &str) -> String {
    format!("{KEY_PREFIX}{username}")
}

/// Sealing keys read from the environment.
///
/// The key is optional so a relay that does not use Lightning Addresses can run
/// without one; when it is missing, LN address records are neither read nor
/// written (fail closed) rather than falling back to plaintext.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct LnAddressKeys {
    current: Option<[u8; 32]>,
    previous: Option<[u8; 32]>,
}

impl LnAddressKeys {
    /// Builds the key set from the environment values, which are 64 hex
    /// characters each (surrounding whitespace is ignored).
    ///
    /// A malformed value is treated as absent so the relay fails closed, and
    /// the mistake is logged once per isolate.
    pub fn from_hex(current: Option<String>, previous: Option<String>) -> Self {
        Self {
            current: parse_hex_key(current, KEY_ENV),
            previous: parse_hex_key(previous, PREVIOUS_KEY_ENV),
        }
    }

    /// Whether a current key is available for reading and writing records.
    pub fn is_configured(&self) -> bool {
        self.current.is_some()
    }

    /// Keys to try when opening a payload, current first then the previous
    /// one during rotation.
    fn candidates(&self) -> impl Iterator<Item = &[u8; 32]> {
        self.current.iter().chain(self.previous.iter())
    }
}

impl std::fmt::Debug for LnAddressKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LnAddressKeys")
            .field("current", &self.current.is_some())
            .field("previous", &self.previous.is_some())
            .finish()
    }
}

/// Why a record could not be sealed or opened.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RecordError {
    /// No usable `LN_ADDRESS_KEY` is configured.
    #[error("LN address key is not configured")]
    MissingKey,
    /// The value does not carry the sealed-record prefix.
    #[error("LN address record is not sealed")]
    NotSealed,
    /// The payload version is newer than this build understands.
    #[error("unsupported LN address record version")]
    UnsupportedVersion,
    /// The payload is not a well-formed record.
    #[error("invalid LN address record")]
    InvalidPayload,
    /// The record belongs to a different username than the key it was read
    /// from.
    #[error("LN address record belongs to another username")]
    UsernameMismatch,
    /// HKDF could not produce a sealing key.
    #[error("LN address key derivation failed")]
    KeyDerivation,
    /// Encryption failed.
    #[error("LN address record could not be encrypted")]
    Encrypt(#[source] RelayError),
    /// Decryption or authentication failed under every configured key.
    #[error("LN address record could not be decrypted")]
    Decrypt(#[source] RelayError),
}

/// Seals `record` into the stored representation.
///
/// # Errors
///
/// Returns [`RecordError::MissingKey`] when no current key is configured, and
/// an encryption error when the crypto backend refuses the payload.
pub fn seal(record: &LnAddressRecord, keys: &LnAddressKeys) -> Result<String, RecordError> {
    let secret = keys.current.ok_or(RecordError::MissingKey)?;
    let key = derive_key(&secret)?;
    let json = serde_json::to_string(record).map_err(|_| RecordError::InvalidPayload)?;
    let payload = encrypt_nip44(&key, &json).map_err(RecordError::Encrypt)?;
    Ok(format!("{SEALED_PREFIX}{payload}"))
}

/// Opens `value`, which must have been stored under [`key_for`]`(username)`.
///
/// Both the current and the previous key are tried, so a rotation does not
/// strand records written by the older key.
///
/// # Errors
///
/// Returns [`RecordError::NotSealed`] when the value lacks the sealed prefix,
/// [`RecordError::MissingKey`] when no key is configured, a decryption error
/// when no key authenticates the payload, and [`RecordError::UsernameMismatch`]
/// when the payload belongs to a different username.
pub fn open_sealed(
    value: &str,
    username: &str,
    keys: &LnAddressKeys,
) -> Result<LnAddressRecord, RecordError> {
    let payload = value
        .strip_prefix(SEALED_PREFIX)
        .ok_or(RecordError::NotSealed)?;

    let mut failure = RecordError::MissingKey;
    for secret in keys.candidates() {
        let key = derive_key(secret)?;
        match decrypt_nip44(&key, payload) {
            // A payload that authenticates but names another user is refused
            // outright: trying the previous key instead would turn a moved
            // row into a silent credential swap.
            Ok(json) => return parse_record(&json, username),
            Err(e) => failure = RecordError::Decrypt(e),
        }
    }
    Err(failure)
}

fn parse_record(json: &str, username: &str) -> Result<LnAddressRecord, RecordError> {
    let record: LnAddressRecord =
        serde_json::from_str(json).map_err(|_| RecordError::InvalidPayload)?;
    if record.version != RECORD_VERSION {
        return Err(RecordError::UnsupportedVersion);
    }
    if record.username != username {
        return Err(RecordError::UsernameMismatch);
    }
    Ok(record)
}

fn derive_key(secret: &[u8; 32]) -> Result<[u8; 32], RecordError> {
    let hkdf = hkdf::Hkdf::<Sha256>::new(None, secret);
    let mut key = [0u8; 32];
    hkdf.expand(KEY_INFO.as_bytes(), &mut key)
        .map_err(|_| RecordError::KeyDerivation)?;
    Ok(key)
}

fn parse_hex_key(value: Option<String>, name: &'static str) -> Option<[u8; 32]> {
    let value = value?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    match hex::decode(trimmed)
        .ok()
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
    {
        Some(key) => Some(key),
        None => {
            log_malformed_key_once(name);
            None
        }
    }
}

/// One warning per variable per isolate: a bad secret is an operator mistake,
/// and replying to every LN address request with a log line would bury it.
fn log_malformed_key_once(name: &str) {
    static CURRENT_WARNED: AtomicBool = AtomicBool::new(false);
    static PREVIOUS_WARNED: AtomicBool = AtomicBool::new(false);
    let warned = if name == PREVIOUS_KEY_ENV {
        &PREVIOUS_WARNED
    } else {
        &CURRENT_WARNED
    };
    if warned.swap(true, Ordering::Relaxed) {
        return;
    }
    crate::log_error!("{name} must be exactly 64 hex characters; LN address storage is disabled");
}

#[cfg(test)]
#[path = "record_test.rs"]
mod record_test;
