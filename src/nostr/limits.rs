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
    /// (96 KiB content, 100 subscriptions per connection). The content
    /// ceiling covers the largest NIP-44 payload (87,472 base64 characters)
    /// with room for the JSON envelope while staying inside the 128 KiB
    /// WebSocket message cap.
    fn default() -> Self {
        Self {
            max_content_length: 98_304,
            max_subscriptions_per_connection: 100,
        }
    }
}

#[cfg(test)]
#[path = "limits_test.rs"]
mod limits_test;
