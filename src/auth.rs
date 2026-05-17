use worker::Env;

#[derive(Debug, PartialEq)]
pub enum AuthError {
    Forbidden,
}

/// The AccessPolicy manages the relay's security policy.
/// It supports a "Private Mode" (using RELAY_SECRET path parameter)
/// and a "Public Mode" (if no secret is configured).
pub struct AccessPolicy {
    expected_secret: Option<String>,
}

impl AccessPolicy {
    pub fn new(expected_secret: Option<String>) -> Self {
        let expected_secret = match expected_secret {
            Some(s) if !s.is_empty() => Some(s),
            _ => None,
        };
        Self { expected_secret }
    }

    pub fn from_env(env: &Env) -> Self {
        Self::new(env.var("RELAY_SECRET").map(|v| v.to_string()).ok())
    }

    /// Checks if the provided secret matches the expected policy.
    pub fn check_access(&self, provided_secret: &str) -> Result<(), AuthError> {
        match &self.expected_secret {
            // Private Mode: Must match exactly in constant-time
            Some(expected) => {
                if constant_time_eq(provided_secret, expected) {
                    Ok(())
                } else {
                    Err(AuthError::Forbidden)
                }
            }
            // Public Mode: Always authorized
            None => Ok(()),
        }
    }
}

/// Constant-time string comparison to prevent timing attacks.
/// Moved from utils.rs to keep security logic localized.
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
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
}
