use super::*;

fn secret(value: &str) -> Option<String> {
    Some(value.to_string())
}

#[test]
fn test_private_mode_requires_the_exact_secret() {
    let policy = AccessPolicy::new(secret("Secret123"), false);
    assert_eq!(policy.check_access("Secret123"), Ok(()));
    for wrong in ["", "secret123", "SECRET123", "Secret1234", "Secret12"] {
        assert_eq!(
            policy.check_access(wrong),
            Err(AuthError::Forbidden),
            "{wrong:?}"
        );
    }
}

/// Nothing configured means closed, not open: a missing or mistyped secret
/// must never turn the relay public.
#[test]
fn test_no_secret_refuses_everything() {
    for configured in [None, secret("")] {
        let policy = AccessPolicy::new(configured, false);
        for provided in ["", "anything"] {
            assert_eq!(
                policy.check_access(provided),
                Err(AuthError::NotConfigured),
                "{provided:?}"
            );
        }
    }
}

#[test]
fn test_public_mode_must_be_asked_for() {
    let policy = AccessPolicy::new(None, true);
    assert_eq!(policy.check_access(""), Ok(()));
    assert_eq!(policy.check_access("anything"), Ok(()));
}

/// With both set, the secret wins: a leftover public flag must not open a
/// relay that has a password.
#[test]
fn test_secret_wins_over_the_public_flag() {
    let policy = AccessPolicy::new(secret("pw"), true);
    assert_eq!(policy.check_access("pw"), Ok(()));
    assert_eq!(policy.check_access(""), Err(AuthError::Forbidden));
}

#[test]
fn test_public_flag_accepts_only_true() {
    assert!(public_flag(Some("true")));
    for value in [
        None,
        Some(""),
        Some("TRUE"),
        Some("True"),
        Some("true "),
        Some("1"),
        Some("yes"),
    ] {
        assert!(!public_flag(value), "{value:?}");
    }
}

#[test]
fn test_constant_time_eq() {
    assert!(constant_time_eq("abc", "abc"));
    assert!(!constant_time_eq("abc", "abd"));
    assert!(!constant_time_eq("abc", "abcd"));
    assert!(constant_time_eq("", ""));
}

#[test]
fn test_constant_time_eq_different_lengths_short() {
    assert!(!constant_time_eq("a", "ab"));
    assert!(!constant_time_eq("ab", "a"));
}

#[test]
fn test_constant_time_eq_all_same_bytes() {
    assert!(constant_time_eq("aaa", "aaa"));
    assert!(!constant_time_eq("aaa", "baa"));
}

#[test]
fn test_constant_time_eq_long_strings() {
    let a = "this is a much longer test string for testing";
    let b = "this is a much longer test string for testing";
    let c = "this is a much longer test string for testing!";
    assert!(constant_time_eq(a, b));
    assert!(!constant_time_eq(a, c));
}

#[test]
fn test_constant_time_eq_unicode() {
    assert!(constant_time_eq("🦀", "🦀"));
    assert!(!constant_time_eq("🦀", "🐡"));
    assert!(constant_time_eq("こんにちは", "こんにちは"));
    assert!(!constant_time_eq("こんにちは", "こんばんは"));
}

#[test]
fn test_constant_time_eq_rejects_many_distinct_secrets() {
    // Comparison folds the SHA-256 digests, so a single differing pair can
    // accidentally leave the accumulator at zero if the combine is broken
    // (e.g. XOR instead of OR). Every distinct pair must still compare unequal.
    for i in 0..64u32 {
        let a = format!("secret-{i}");
        let b = format!("secret-{}", i + 1);
        assert!(!constant_time_eq(&a, &b), "{a} must not equal {b}");
    }
}
