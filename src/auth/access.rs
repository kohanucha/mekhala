//! Cloudflare Access: the team's signing keys and the token Access forwards.
//!
//! Access already refuses unauthenticated visitors before a request reaches
//! the Worker, but the admin app keeps its own gate. A policy that is later
//! widened, an application that is re-scoped to another path, or a request
//! that arrives through a different hostname must not turn into an open
//! registration endpoint.
//!
//! Verification follows the parts of the Access JWT document
//! (<https://developers.cloudflare.com/cloudflare-one/identity/authorization-cookie/validating-json/>)
//! that decide whether the token belongs to this application at this moment:
//! the signature against the team's published keys, `iss`, `aud`, `type`,
//! `exp` and `nbf`.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rsa::pkcs1v15::{Signature, VerifyingKey};
use rsa::sha2::Sha256;
use rsa::signature::Verifier;
use rsa::{BoxedUint, PublicKey};
use serde::Deserialize;

/// Longest token accepted, in bytes. An Access token is a few kilobytes; the
/// cap means a crafted header is refused before it is decoded.
pub const MAX_TOKEN_LEN: usize = 8 * 1024;

/// Longest key document accepted, in bytes.
pub const MAX_JWKS_LEN: usize = 64 * 1024;

/// Keys kept from one document. A team publishes two or three.
const MAX_KEYS: usize = 32;

/// Access application tokens carry `"type": "app"`. An account-level token is
/// not bound to this application, so it is refused.
const ACCESS_TYPE: &str = "app";

/// Why a token could not be accepted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JwtError {
    /// The token or key document is larger than this relay parses.
    #[error("token is too large")]
    TooLarge,
    /// The value is not a three-part JWT or does not decode.
    #[error("token is malformed")]
    Malformed,
    /// The token was signed with something other than RS256.
    #[error("token algorithm is not RS256")]
    UnsupportedAlgorithm,
    /// No published key matches the token's key id and signature.
    #[error("token signature does not verify")]
    InvalidSignature,
    /// `exp` has passed.
    #[error("token has expired")]
    Expired,
    /// `nbf` is in the future.
    #[error("token is not valid yet")]
    NotYetValid,
    /// `aud` does not name this application.
    #[error("token was issued for another audience")]
    WrongAudience,
    /// `type` is not `app`.
    #[error("token is not an Access application token")]
    WrongType,
    /// `iss` is another team's domain.
    #[error("token was issued by another team")]
    WrongIssuer,
    /// The key document carried no RSA signing key this relay can use.
    #[error("no usable key in the key document")]
    NoKeys,
}

/// What an accepted token tells the admin channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessClaims {
    /// Who Access authenticated: an email address, or a service token's
    /// client id.
    pub subject: Option<String>,
}

/// The keys from one `…/cdn-cgi/access/certs` document.
#[derive(Debug, Clone, Default)]
pub struct AccessKeys {
    keys: Vec<SigningKey>,
}

#[derive(Debug, Clone)]
struct SigningKey {
    /// Key id the token may name; `None` for a document without ids.
    kid: Option<String>,
    key: PublicKey,
}

impl AccessKeys {
    /// Parses `document`, keeping the RSA keys that can verify RS256.
    ///
    /// Entries of another key type or algorithm are skipped rather than
    /// failing the whole set: the team's document may publish keys for other
    /// purposes, and only one of them has to match.
    ///
    /// # Errors
    ///
    /// Returns [`JwtError::TooLarge`] for an oversized document,
    /// [`JwtError::Malformed`] when it is not a key document, and
    /// [`JwtError::NoKeys`] when nothing usable is left.
    pub fn from_jwks(document: &str) -> Result<Self, JwtError> {
        if document.len() > MAX_JWKS_LEN {
            return Err(JwtError::TooLarge);
        }
        let parsed: JwkSet = serde_json::from_str(document).map_err(|_| JwtError::Malformed)?;

        let mut keys = Vec::new();
        for jwk in parsed.keys.into_iter().take(MAX_KEYS) {
            if jwk.kty != "RSA" {
                continue;
            }
            match jwk.alg.as_deref() {
                None | Some("RS256") => {}
                Some(_) => continue,
            }
            let (Some(n), Some(e)) = (jwk.n, jwk.e) else {
                continue;
            };
            let modulus = decode(&n)?;
            let exponent = decode(&e)?;
            let Ok(key) = PublicKey::new(
                BoxedUint::from_be_slice_vartime(&modulus),
                BoxedUint::from_be_slice_vartime(&exponent),
            ) else {
                // A key this platform refuses (too small, malformed modulus)
                // cannot verify anything; the remaining keys still can.
                continue;
            };
            keys.push(SigningKey { kid: jwk.kid, key });
        }

        if keys.is_empty() {
            return Err(JwtError::NoKeys);
        }
        Ok(Self { keys })
    }

    /// Verifies `token` was signed by one of these keys for `audience` on
    /// `issuer`, and is valid at `now`.
    ///
    /// The signature is checked before the claims are read, so a token whose
    /// content was edited never reaches the claim checks.
    ///
    /// # Errors
    ///
    /// Returns the first check that fails: [`JwtError::Malformed`],
    /// [`JwtError::UnsupportedAlgorithm`], [`JwtError::InvalidSignature`],
    /// [`JwtError::WrongIssuer`], [`JwtError::WrongAudience`],
    /// [`JwtError::WrongType`], [`JwtError::Expired`] or
    /// [`JwtError::NotYetValid`].
    pub fn verify(
        &self,
        token: &str,
        issuer: &str,
        audience: &str,
        now: u64,
    ) -> Result<AccessClaims, JwtError> {
        if token.len() > MAX_TOKEN_LEN {
            return Err(JwtError::TooLarge);
        }

        let mut parts = token.split('.');
        let header_b64 = parts.next().ok_or(JwtError::Malformed)?;
        let payload_b64 = parts.next().ok_or(JwtError::Malformed)?;
        let signature_b64 = parts.next().ok_or(JwtError::Malformed)?;
        if parts.next().is_some() {
            return Err(JwtError::Malformed);
        }

        let header: Header = decode_json(header_b64)?;
        if header.alg != "RS256" {
            return Err(JwtError::UnsupportedAlgorithm);
        }

        let signed = format!("{header_b64}.{payload_b64}");
        let raw = decode(signature_b64)?;
        let signature = Signature::try_from(raw.as_slice()).map_err(|_| JwtError::Malformed)?;

        let candidates: Vec<&SigningKey> = match header.kid.as_deref() {
            Some(kid) => self
                .keys
                .iter()
                .filter(|k| k.kid.as_deref() == Some(kid))
                .collect(),
            None => self.keys.iter().collect(),
        };
        let mut verified = false;
        for candidate in candidates {
            let verifier = VerifyingKey::<Sha256>::new(candidate.key.clone());
            if verifier.verify(signed.as_bytes(), &signature).is_ok() {
                verified = true;
                break;
            }
        }
        if !verified {
            return Err(JwtError::InvalidSignature);
        }

        let claims: Claims = decode_json(payload_b64)?;
        if claims.iss != issuer {
            return Err(JwtError::WrongIssuer);
        }
        if !claims.aud.contains(audience) {
            return Err(JwtError::WrongAudience);
        }
        if claims.kind != ACCESS_TYPE {
            return Err(JwtError::WrongType);
        }
        if claims.exp <= now {
            return Err(JwtError::Expired);
        }
        if claims.nbf.is_some_and(|nbf| nbf > now) {
            return Err(JwtError::NotYetValid);
        }

        Ok(AccessClaims {
            subject: claims.common_name,
        })
    }
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    #[serde(default)]
    kid: Option<String>,
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    aud: Audience,
    exp: u64,
    #[serde(default)]
    nbf: Option<u64>,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    common_name: Option<String>,
}

/// `aud` is an array in current tokens and a bare string in older ones.
#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Audience {
    fn contains(&self, audience: &str) -> bool {
        match self {
            Self::One(value) => value == audience,
            Self::Many(values) => values.iter().any(|value| value == audience),
        }
    }
}

#[derive(Deserialize)]
struct JwkSet {
    keys: Vec<Jwk>,
}

#[derive(Deserialize)]
struct Jwk {
    kty: String,
    #[serde(default)]
    alg: Option<String>,
    #[serde(default)]
    kid: Option<String>,
    #[serde(default)]
    n: Option<String>,
    #[serde(default)]
    e: Option<String>,
}

fn decode(value: &str) -> Result<Vec<u8>, JwtError> {
    URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .map_err(|_| JwtError::Malformed)
}

fn decode_json<T: for<'de> Deserialize<'de>>(value: &str) -> Result<T, JwtError> {
    serde_json::from_slice(&decode(value)?).map_err(|_| JwtError::Malformed)
}

#[cfg(test)]
#[path = "access_test.rs"]
mod access_test;
