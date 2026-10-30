use serde::{Deserialize, Serialize};

/// Relay-enforced per-connection ceilings, parsed from environment
/// configuration (see `CloudflareConfig`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Limits {
    /// Maximum serialized `content` length in bytes accepted per event.
    pub max_content_length: usize,
    /// Maximum number of subscriptions a single connection may hold.
    pub max_subscriptions_per_connection: usize,
}

impl Limits {
    /// Explicit construction from configuration values.
    pub fn new(max_content_length: usize, max_subscriptions_per_connection: usize) -> Self {
        Self {
            max_content_length,
            max_subscriptions_per_connection,
        }
    }
}

impl Default for Limits {
    /// Conservative defaults used when no configuration is provided
    /// (64 KiB content, 100 subscriptions per connection).
    fn default() -> Self {
        Self {
            max_content_length: 65536,
            max_subscriptions_per_connection: 100,
        }
    }
}

#[cfg(test)]
#[path = "limits_test.rs"]
mod limits_test;