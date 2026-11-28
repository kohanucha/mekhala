//! Access control: a relay with a secret is private, one without is public.

use sha2::{Digest, Sha256};

/// Auth failure returned by [`AccessPolicy::check_access`].
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum AuthError {
    /// The secret in the path is missing or wrong.
    #[error("forbidden: missing or invalid relay secret")]
    Forbidden,
}

/// Access control for the relay's HTTP endpoints.
///
/// No secret (or an empty one) runs a public relay at the root path; setting
/// one makes the relay private to whoever has the secret.
pub struct AccessPolicy {
    secret: Option<String>,
}

impl AccessPolicy {
    pub fn new(secret: Option<String>) -> Self {
        Self {
            secret: secret.filter(|s| !s.is_empty()),
        }
    }

    /// Whether anyone may use the relay.
    pub fn is_public(&self) -> bool {
        self.secret.is_none()
    }

    /// Authorizes a request by the secret in its path, compared in constant
    /// time.
    ///
    /// # Errors
    ///
    /// [`AuthError::Forbidden`] when the relay has a secret and
    /// `provided_secret` does not match it.
    pub fn check_access(&self, provided_secret: &str) -> Result<(), AuthError> {
        match &self.secret {
            None => Ok(()),
            Some(expected) if constant_time_eq(provided_secret, expected) => Ok(()),
            Some(_) => Err(AuthError::Forbidden),
        }
    }
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
