use super::*;

#[test]
fn test_limits_default() {
    let limits = Limits::default();
    assert_eq!(limits.max_content_length, 98_304);
    assert_eq!(limits.max_subscriptions_per_connection, 100);
}

/// The default content ceiling must cover the largest NIP-44 v2 payload
/// (87,472 base64 characters) or wallets cannot publish big responses.
#[test]
fn test_default_content_limit_covers_nip44_payload() {
    let limits = Limits::default();
    assert!(limits.max_content_length >= 87_472);
}

#[test]
fn test_limits_new() {
    let limits = Limits::new(16384, 50);
    assert_eq!(limits.max_content_length, 16384);
    assert_eq!(limits.max_subscriptions_per_connection, 50);
}

#[test]
fn test_limits_serde_roundtrip() {
    let limits = Limits::new(99999, 42);
    let json = serde_json::to_value(limits).unwrap();
    let deserialized: Limits = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized.max_content_length, 99999);
    assert_eq!(deserialized.max_subscriptions_per_connection, 42);
}

#[test]
fn test_limits_default_key_and_tag_ceilings() {
    let limits = Limits::default();
    assert_eq!(limits.max_filter_items, 10);
    assert_eq!(limits.max_event_tags, 10);
    assert_eq!(limits.max_routing_keys_per_connection, 1_000);
}

#[test]
fn test_limits_new_keeps_key_and_tag_defaults() {
    let limits = Limits::new(16384, 50);
    let defaults = Limits::default();
    assert_eq!(limits.max_filter_items, defaults.max_filter_items);
    assert_eq!(limits.max_event_tags, defaults.max_event_tags);
    assert_eq!(
        limits.max_routing_keys_per_connection,
        defaults.max_routing_keys_per_connection
    );
}

#[test]
fn test_limits_serde_roundtrip_preserves_key_and_tag_ceilings() {
    let limits = Limits {
        max_filter_items: 3,
        max_event_tags: 4,
        max_routing_keys_per_connection: 5,
        ..Limits::default()
    };
    let json = serde_json::to_value(limits).unwrap();
    let deserialized: Limits = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized.max_filter_items, 3);
    assert_eq!(deserialized.max_event_tags, 4);
    assert_eq!(deserialized.max_routing_keys_per_connection, 5);
}
