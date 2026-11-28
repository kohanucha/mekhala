use super::*;

fn secret(value: &str) -> Option<String> {
    Some(value.to_string())
}

#[test]
fn test_private_mode_requires_the_exact_secret() {
    let policy = AccessPolicy::new(secret("Secret123"));
    assert!(!policy.is_public());
    assert_eq!(policy.check_access("Secret123"), Ok(()));
    for wrong in ["", "secret123", "SECRET123", "Secret1234", "Secret12"] {
        assert_eq!(
            policy.check_access(wrong),
            Err(AuthError::Forbidden),
            "{wrong:?}"
        );
    }
}

/// An empty secret is no secret: it must not lock everyone out by matching
/// only the empty path.
#[test]
fn test_no_secret_runs_a_public_relay() {
    for configured in [None, secret("")] {
        let policy = AccessPolicy::new(configured.clone());
        assert!(policy.is_public(), "{configured:?}");
        for provided in ["", "anything"] {
            assert_eq!(policy.check_access(provided), Ok(()), "{provided:?}");
        }
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
