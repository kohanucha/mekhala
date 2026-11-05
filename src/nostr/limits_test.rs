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
