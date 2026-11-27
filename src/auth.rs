//! Access control: the relay secret, compared in constant time, or an explicit
//! public mode. With neither, every request is refused.

use sha2::{Digest, Sha256};

/// Auth failure returned by [`AccessPolicy::check_access`].
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum AuthError {
    /// The secret in the path is missing or wrong.
    #[error("forbidden: missing or invalid relay secret")]
    Forbidden,
    /// No secret is configured and public mode was not asked for, so every
    /// request is refused.
    #[error("refused: no relay secret is configured and public mode is off")]
    NotConfigured,
}

/// Who may use the relay.
enum Access {
    /// The path must carry this secret.
    Private(String),
    /// Anyone, because the operator asked for it.
    Public,
    /// No secret and no public flag: nobody.
    NotConfigured,
}

/// Access control for the relay's HTTP endpoints.
///
/// A missing secret closes the relay rather than opening it: public mode
/// must be asked for explicitly, so a secret that was never set, was set
/// empty or was set under a mistyped name cannot make the relay public.
pub struct AccessPolicy {
    access: Access,
}

impl AccessPolicy {
    /// A non-empty `secret` selects private mode and wins over `public`;
    /// otherwise `public` decides between public mode and refusing everything.
    pub fn new(secret: Option<String>, public: bool) -> Self {
        let access = match secret {
            Some(secret) if !secret.is_empty() => Access::Private(secret),
            _ if public => Access::Public,
            _ => Access::NotConfigured,
        };
        Self { access }
    }

    /// Authorizes a request by the secret in its path, compared in constant
    /// time.
    ///
    /// # Errors
    ///
    /// [`AuthError::Forbidden`] when the secret does not match, and
    /// [`AuthError::NotConfigured`] for every request when there is neither a
    /// secret nor public mode.
    pub fn check_access(&self, provided_secret: &str) -> Result<(), AuthError> {
        match &self.access {
            Access::Private(expected) if constant_time_eq(provided_secret, expected) => Ok(()),
            Access::Private(_) => Err(AuthError::Forbidden),
            Access::Public => Ok(()),
            Access::NotConfigured => Err(AuthError::NotConfigured),
        }
    }
}

/// Reads the public-mode flag. Only the exact value `true` turns it on, so a
/// typo leaves the relay closed.
pub fn public_flag(value: Option<&str>) -> bool {
    value == Some("true")
}

/// Constant-time string comparison to prevent timing attacks.
/// Hashes inputs to ensure comparison time is independent of input lengths.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let hash_a = Sha256::digest(a.as_bytes());
    let hash_b = Sha256::digest(b.as_bytes());

    hash_a
        .iter()
        .zip(hash_b.iter())
        .fold(0, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
#[path = "auth_test.rs"]
mod auth_test;
