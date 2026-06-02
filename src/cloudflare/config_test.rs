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
    assert_eq!(CloudflareConfig::parse_opt(Some("not-a-number".into()), 42), 42);
}

#[test]
fn test_parse_opt_zero() {
    assert_eq!(CloudflareConfig::parse_opt(Some("0".into()), 42), 0);
}

#[test]
fn test_parse_opt_empty_uses_default() {
    assert_eq!(CloudflareConfig::parse_opt(Some("".into()), 42), 42);
}

#[test]
fn test_from_env_defaults() {
}
