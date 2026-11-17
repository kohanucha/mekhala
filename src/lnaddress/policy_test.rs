use super::*;

#[test]
fn test_zero_minimum_falls_back_to_defaults() {
    assert_eq!(SendableLimits::new(0, 500), SendableLimits::default());
}

#[test]
fn test_zero_maximum_falls_back_to_defaults() {
    assert_eq!(SendableLimits::new(500, 0), SendableLimits::default());
}

#[test]
fn test_zero_range_falls_back_to_defaults() {
    assert_eq!(SendableLimits::new(0, 0), SendableLimits::default());
}

#[test]
fn test_inverted_range_falls_back_to_defaults() {
    assert_eq!(SendableLimits::new(2_000, 1_000), SendableLimits::default());
}

#[test]
fn test_defaults_are_the_documented_values() {
    let limits = SendableLimits::default();
    assert_eq!(limits.min_msat(), 1_000);
    assert_eq!(limits.max_msat(), 100_000_000);
}

#[test]
fn test_new_keeps_a_valid_range() {
    let limits = SendableLimits::new(2_000, 50_000);
    assert_eq!(limits.min_msat(), 2_000);
    assert_eq!(limits.max_msat(), 50_000);
}

#[test]
fn test_single_amount_range_is_valid() {
    let limits = SendableLimits::new(5_000, 5_000);
    assert_eq!(limits.min_msat(), 5_000);
    assert_eq!(limits.max_msat(), 5_000);
    assert!(validate_amount(5_000, &limits).is_ok());
}

#[test]
fn test_validate_accepts_both_bounds() {
    let limits = SendableLimits::new(1_000, 1_000_000);
    assert!(validate_amount(1_000, &limits).is_ok());
    assert!(validate_amount(1_000_000, &limits).is_ok());
}

#[test]
fn test_validate_accepts_amount_inside_range() {
    let limits = SendableLimits::new(1_000, 1_000_000);
    assert!(validate_amount(21_000, &limits).is_ok());
}

#[test]
fn test_validate_rejects_below_minimum() {
    let limits = SendableLimits::new(1_000, 1_000_000);
    assert_eq!(
        validate_amount(999, &limits),
        Err(AmountError::BelowMinimum(1_000))
    );
}

#[test]
fn test_validate_rejects_above_maximum() {
    let limits = SendableLimits::new(1_000, 1_000_000);
    assert_eq!(
        validate_amount(1_000_001, &limits),
        Err(AmountError::AboveMaximum(1_000_000))
    );
}

#[test]
fn test_zero_amount_is_below_the_default_minimum() {
    assert_eq!(
        validate_amount(0, &SendableLimits::default()),
        Err(AmountError::BelowMinimum(DEFAULT_MIN_SENDABLE_MSAT))
    );
}

#[test]
fn test_amount_error_messages_name_the_bound() {
    assert_eq!(
        AmountError::BelowMinimum(1_000).to_string(),
        "Amount below minimum (1000 msat)"
    );
    assert_eq!(
        AmountError::AboveMaximum(2_000).to_string(),
        "Amount above maximum (2000 msat)"
    );
}

#[test]
fn test_policy_sanitises_the_range() {
    let policy = LnAddressPolicy::new(0, 0, 5);
    assert_eq!(policy.limits, SendableLimits::default());
    assert_eq!(policy.callback_rate_limit_per_minute, 5);
}

#[test]
fn test_policy_keeps_rate_limit_zero() {
    let policy = LnAddressPolicy::new(1_000, 2_000, 0);
    assert_eq!(policy.callback_rate_limit_per_minute, 0);
    assert_eq!(policy.limits.min_msat(), 1_000);
}
