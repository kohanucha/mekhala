use serde::{Deserialize, Serialize};

/// Longest subscription id accepted in a `REQ`. Ids are kept in memory and in
/// the persisted connection snapshot, so they need a ceiling.
pub const MAX_SUB_ID_LENGTH: usize = 128;

/// Relay-enforced ceilings, parsed from environment configuration (see
/// `CloudflareConfig`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Limits {
    /// Maximum serialized `content` length in bytes accepted per event.
    pub max_content_length: usize,
    /// Maximum number of subscriptions a single connection may hold.
    pub max_subscriptions_per_connection: usize,
    /// Maximum number of values in one filter field (`ids`, `authors`,
    /// `kinds`, `#p`, `#e`).
    pub max_filter_items: usize,
    /// Maximum number of tags accepted on one event.
    pub max_event_tags: usize,
    /// Maximum number of routing keys (`pk:`, `eid:`, `etag:`) one
    /// connection may reference across all of its subscriptions.
    ///
    /// Every key costs a storage read and a write on every `subscribe()`, so
    /// without this bound a single `REQ` could ask for tens of thousands.
    pub max_routing_keys_per_connection: usize,
    /// Maximum number of wallet-service info events kept in the cache
    /// (`info:` family). Each one also carries an `infoid:` reverse mapping
    /// and one `info-p:` alias per addressed app, so the number of keys is a
    /// multiple of this. `0` disables the cap.
    ///
    /// The cache exists so hibernated connections can still answer a
    /// discovery `REQ`; it is not meant to be an archive of every wallet that
    /// ever published.
    pub max_info_events: usize,
    /// How long a cached info event is served before it is treated as absent
    /// and deleted. `0` disables expiry.
    pub info_ttl_seconds: u64,
}

impl Limits {
    /// Explicit construction of the two original ceilings; the key/tag
    /// ceilings keep their defaults.
    pub fn new(max_content_length: usize, max_subscriptions_per_connection: usize) -> Self {
        Self {
            max_content_length,
            max_subscriptions_per_connection,
            ..Self::default()
        }
    }
}

impl Default for Limits {
    /// Conservative defaults used when no configuration is provided (96 KiB
    /// content, 100 subscriptions per connection, 10 values per filter field,
    /// 10 tags per event, 1000 routing keys per connection, 1000 cached info
    /// events kept for 30 days). The content ceiling covers the largest
    /// NIP-44 payload (87,472 base64 characters) with room for the JSON
    /// envelope while staying inside the 128 KiB WebSocket message cap.
    fn default() -> Self {
        Self {
            max_content_length: 98_304,
            max_subscriptions_per_connection: 100,
            max_filter_items: 10,
            max_event_tags: 10,
            max_routing_keys_per_connection: 1_000,
            max_info_events: 1_000,
            info_ttl_seconds: 2_592_000,
        }
    }
}

#[cfg(test)]
#[path = "limits_test.rs"]
mod limits_test;
