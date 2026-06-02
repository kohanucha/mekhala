//! Access control: relay secret authentication via constant-time comparison.

use sha2::{Sha256, Digest};

#[derive(Debug, PartialEq)]
pub enum AuthError {
    Forbidden,
}

/// The AccessPolicy manages the relay's security policy.
/// It supports a "Private Mode" (using RELAY_SECRET path parameter)
/// and a "Public Mode" (if no secret is configured).
pub struct AccessPolicy {
    expected_secret: Option<String>,
}

impl AccessPolicy {
    pub fn new(expected_secret: Option<String>) -> Self {
        let expected_secret = match expected_secret {
            Some(s) if !s.is_empty() => Some(s),
            _ => None,
        };
        Self { expected_secret }
    }

    /// Checks if the provided secret matches the expected policy.
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
