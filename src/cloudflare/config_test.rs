use super::*;

#[test]
fn test_parse_opt_missing() {
    assert_eq!(CloudflareConfig::parse_opt(None, 42), 42);
}

#[test]
fn test_parse_opt_valid() {
    assert_eq!(CloudflareConfig::parse_opt(Some("100".into()), 42), 100);
}

#[test]
fn test_parse_opt_invalid_uses_default() {
    assert_eq!(
        CloudflareConfig::parse_opt(Some("not-a-number".into()), 42),
        42
    );
}

#[test]
fn test_parse_opt_zero() {
    assert_eq!(CloudflareConfig::parse_opt(Some("0".into()), 42), 0);
}

#[test]
fn test_parse_opt_empty_uses_default() {
    assert_eq!(CloudflareConfig::parse_opt(Some(String::new()), 42), 42);
}

#[test]
fn test_from_env_defaults() {}

#[test]
fn test_parse_opt_u64_valid() {
    assert_eq!(
        CloudflareConfig::parse_opt_u64(Some("2592000".into()), 42),
        2_592_000
    );
}

#[test]
fn test_parse_opt_u64_invalid_uses_default() {
    assert_eq!(
        CloudflareConfig::parse_opt_u64(Some("not-a-number".into()), 42),
        42
    );
}

#[test]
fn test_parse_opt_u64_zero_is_kept() {
    assert_eq!(CloudflareConfig::parse_opt_u64(Some("0".into()), 42), 0);
}

#[test]
fn test_parse_opt_u32_valid() {
    assert_eq!(CloudflareConfig::parse_opt_u32(Some("10".into()), 42), 10);
}

#[test]
fn test_parse_opt_u32_invalid_uses_default() {
    assert_eq!(CloudflareConfig::parse_opt_u32(Some("ten".into()), 42), 42);
}

#[test]
fn test_parse_opt_u32_zero_disables() {
    assert_eq!(CloudflareConfig::parse_opt_u32(Some("0".into()), 42), 0);
}
