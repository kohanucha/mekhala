//! Access control: relay secret authentication via constant-time comparison.

use sha2::{Sha256, Digest};

/// Auth failure returned by [`AccessPolicy::check_access`].
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum AuthError {
    /// Secret missing or wrong (private mode).
    #[error("forbidden: missing or invalid relay secret")]
    Forbidden,
}

/// Access control for the relay's HTTP endpoints.
///
/// Supports "public mode" (no secret configured — everything passes) and
/// "private mode" (a `RELAY_SECRET` must be supplied), verified with a
/// constant-time comparison.
pub struct AccessPolicy {
    expected_secret: Option<String>,
}

impl AccessPolicy {
    /// Configures the policy; an empty-string secret counts as "no secret"
    /// (public mode).
    pub fn new(expected_secret: Option<String>) -> Self {
        let expected_secret = match expected_secret {
            Some(s) if !s.is_empty() => Some(s),
            _ => None,
        };
        Self { expected_secret }
    }

    /// Authorizes a request: "public mode" (no secret configured) always
    /// passes; "private mode" requires a constant-time match of
    /// `provided_secret` against the configured relay secret.
    ///
    /// # Errors
    ///
    /// Returns [`AuthError::Forbidden`] only in private mode, when
    /// `provided_secret` does not match the configured relay secret (or was
    /// left empty). Public mode always succeeds.
    pub fn check_access(&self, provided_secret: &str) -> Result<(), AuthError> {
        match &self.expected_secret {
            // Private Mode: Must match exactly in constant-time
            Some(expected) => {
                if constant_time_eq(provided_secret, expected) {
                    Ok(())
                } else {
                    Err(AuthError::Forbidden)
                }
            }
            // Public Mode: Always authorized
            None => Ok(()),
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
