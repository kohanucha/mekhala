//! Turning an admin submission into a stored registration.
//!
//! The admin channel accepts a username and a wallet connection string. This
//! module decides whether the pair is usable and which wallet capabilities the
//! relay may rely on afterwards. It performs no I/O: the caller runs the
//! `get_info` round trip and hands the result here, so the rules are tested
//! without a wallet.

use crate::lnaddress::record::LnAddressRecord;
use crate::lnaddress::validation::is_valid_username;
use crate::nostr::nip_47::{NwcClient, NwcUri};
use serde_json::Value;

/// Longest connection string accepted, in bytes.
pub const MAX_NWC_URI_LEN: usize = 1024;

/// Largest admin submission accepted, in bytes: the two fields, their JSON
/// wrapper, and room to spare. Anything larger is not a registration.
pub const MAX_SUBMISSION_LEN: usize = 4 * 1024;

/// How many granted methods a record keeps.
pub const MAX_METHODS: usize = 32;

/// Longest method name kept, in bytes.
pub const MAX_METHOD_LEN: usize = 32;

/// The capability a Lightning Address needs: minting the invoice a payer is
/// asked to settle.
pub const REQUIRED_METHOD: &str = "pay_invoice";

/// A submission that passed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    /// The Lightning Address username.
    pub username: String,
    /// The wallet connection string, as submitted (trimmed).
    pub nwc_uri: String,
    /// The wallet's public key, taken from the connection string.
    pub wallet_pubkey: String,
}

/// Why a submission was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistrationError {
    /// The username breaks the shape every other LN address path enforces.
    #[error("username is not valid")]
    Username,
    /// The connection string is not a usable `nostr+walletconnect` URI.
    #[error("connection string is not a nostr+walletconnect URI")]
    NotWalletConnect,
    /// The connection string is longer than this relay stores.
    #[error("connection string is too long")]
    TooLong,
    /// The wallet does not grant the method the relay needs.
    #[error("wallet does not grant pay_invoice")]
    MissingScope,
}

/// Validates one submission.
///
/// The username rule is the same one `.well-known` and the callback use, so a
/// stored name is always a name those paths can resolve. The connection string
/// is decoded far enough to prove the key material is usable: a URI that
/// parses but carries a malformed wallet key or secret would be stored and
/// then fail on every invoice.
///
/// # Errors
///
/// Returns [`RegistrationError::Username`],
/// [`RegistrationError::TooLong`] or
/// [`RegistrationError::NotWalletConnect`].
pub fn parse(username: &str, nwc_uri: &str) -> Result<Registration, RegistrationError> {
    let nwc_uri = nwc_uri.trim();
    if !is_valid_username(username) {
        return Err(RegistrationError::Username);
    }
    if nwc_uri.len() > MAX_NWC_URI_LEN {
        return Err(RegistrationError::TooLong);
    }
    let uri = NwcUri::from_uri(nwc_uri).map_err(|_| RegistrationError::NotWalletConnect)?;
    NwcClient::new(uri.clone()).map_err(|_| RegistrationError::NotWalletConnect)?;

    Ok(Registration {
        username: username.to_owned(),
        nwc_uri: nwc_uri.to_owned(),
        wallet_pubkey: uri.wallet_pubkey,
    })
}

/// Reads the `methods` array out of a `get_info` result.
///
/// Only names that look like NIP-47 methods are kept, in the order the wallet
/// reported them, deduplicated and bounded. A wallet that omits `pay_invoice`
/// is refused: the address would answer payers with an error it cannot
/// explain, and the operator would only find out from a failed payment.
///
/// # Errors
///
/// Returns [`RegistrationError::MissingScope`] when `result` carries no
/// methods or does not include [`REQUIRED_METHOD`].
pub fn granted_methods(result: &Value) -> Result<Vec<String>, RegistrationError> {
    let reported = result
        .get("methods")
        .and_then(Value::as_array)
        .ok_or(RegistrationError::MissingScope)?;

    let mut granted: Vec<String> = Vec::new();
    for value in reported {
        let Some(name) = value.as_str() else {
            continue;
        };
        // Method names are lowercase snake_case; anything else is junk a
        // wallet put in the array, not a capability the relay can call.
        let well_formed = !name.is_empty()
            && name.len() <= MAX_METHOD_LEN
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !well_formed || granted.iter().any(|seen| seen == name) {
            continue;
        }
        if granted.len() == MAX_METHODS {
            break;
        }
        granted.push(name.to_owned());
    }

    if !granted.iter().any(|name| name == REQUIRED_METHOD) {
        return Err(RegistrationError::MissingScope);
    }
    Ok(granted)
}

/// What the admin listing shows for one stored registration.
///
/// Enough to recognise the row and see what the wallet granted, and nothing
/// that spends: the connection string and its secret never leave the record.
pub fn summary(record: &LnAddressRecord) -> Value {
    let mut view = serde_json::json!({ "username": record.username });
    if let Ok(uri) = NwcUri::from_uri(&record.nwc_uri) {
        view["wallet_pubkey"] = Value::String(uri.wallet_pubkey);
    }
    if let Some(methods) = record.methods.as_ref() {
        view["methods"] = serde_json::json!(methods);
    }
    view
}

#[cfg(test)]
#[path = "registration_test.rs"]
mod registration_test;
