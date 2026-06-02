use super::*;

#[test]
fn test_access_policy_public_mode() {
    let policy = AccessPolicy { expected_secret: None };
    assert_eq!(policy.check_access(""), Ok(()));
    assert_eq!(policy.check_access("any-secret"), Ok(()));
}

#[test]
fn test_access_policy_private_mode() {
    let policy = AccessPolicy { expected_secret: Some("secret123".into()) };

    // Correct secret
    assert_eq!(policy.check_access("secret123"), Ok(()));

    // Incorrect secrets
    assert_eq!(policy.check_access("wrong"), Err(AuthError::Forbidden));
    assert_eq!(policy.check_access(""), Err(AuthError::Forbidden));
    assert_eq!(policy.check_access("secret1234"), Err(AuthError::Forbidden));
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
fn test_access_policy_from_env_treated_as_none() {
    let policy = AccessPolicy { expected_secret: Some("".into()) };
    assert_eq!(policy.check_access(""), Ok(()));
    assert_eq!(policy.check_access("any"), Err(AuthError::Forbidden));
}

#[test]
fn test_access_policy_empty_string_as_none() {
    let policy = AccessPolicy { expected_secret: None };
    assert_eq!(policy.check_access(""), Ok(()));
    assert_eq!(policy.check_access("anything"), Ok(()));
}

#[test]
fn test_access_policy_private_mode_case_sensitive() {
    let policy = AccessPolicy { expected_secret: Some("Secret123".into()) };
    assert_eq!(policy.check_access("Secret123"), Ok(()));
    assert_eq!(policy.check_access("secret123"), Err(AuthError::Forbidden));
    assert_eq!(policy.check_access("SECRET123"), Err(AuthError::Forbidden));
}

#[test]
fn test_access_policy_private_mode_similar() {
    let policy = AccessPolicy { expected_secret: Some("test-secret-123".into()) };
    assert_eq!(policy.check_access("test-secret-123"), Ok(()));
    assert_eq!(policy.check_access("test-secret-124"), Err(AuthError::Forbidden));
    assert_eq!(policy.check_access("test-secret-12"), Err(AuthError::Forbidden));
    assert_eq!(policy.check_access("test-secret123"), Err(AuthError::Forbidden));
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
fn test_access_policy_new_some() {
    let policy = AccessPolicy::new(Some("mypass".into()));
    assert_eq!(policy.check_access("mypass"), Ok(()));
    assert_eq!(policy.check_access("wrong"), Err(AuthError::Forbidden));
}

#[test]
fn test_access_policy_new_none() {
    let policy = AccessPolicy::new(None);
    assert_eq!(policy.check_access("anything"), Ok(()));
}

#[test]
fn test_access_policy_new_empty_string() {
    let policy = AccessPolicy::new(Some("".into()));
    assert_eq!(policy.check_access("anything"), Ok(()));
}
