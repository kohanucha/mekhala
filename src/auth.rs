use worker::Env;

/// The Authenticator manages the relay's security policy.
/// It supports a "Private Mode" (using RELAY_SECRET path parameter)
/// and a "Public Mode" (if no secret is configured).
pub struct Authenticator {
    expected_secret: Option<String>,
}

impl Authenticator {
    pub fn from_env(env: &Env) -> Self {
        let expected_secret = env.var("RELAY_SECRET").map(|v| v.to_string()).ok();
        
        // Treat an empty secret as None (Public Mode)
        let expected_secret = match expected_secret {
            Some(s) if !s.is_empty() => Some(s),
            _ => None,
        };

        Self { expected_secret }
    }

    /// Checks if the provided secret matches the expected policy.
    pub fn is_authorized(&self, provided_secret: &str) -> bool {
        match &self.expected_secret {
            // Private Mode: Must match exactly in constant-time
            Some(expected) => constant_time_eq(provided_secret, expected),
            // Public Mode: Always authorized
            None => true,
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
    fn test_authenticator_public_mode() {
        let auth = Authenticator { expected_secret: None };
        assert!(auth.is_authorized(""));
        assert!(auth.is_authorized("any-secret"));
    }

    #[test]
    fn test_authenticator_private_mode() {
        let auth = Authenticator { expected_secret: Some("secret123".into()) };
        
        // Correct secret
        assert!(auth.is_authorized("secret123"));
        
        // Incorrect secrets
        assert!(!auth.is_authorized("wrong"));
        assert!(!auth.is_authorized(""));
        assert!(!auth.is_authorized("secret1234")); // Length mismatch
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(constant_time_eq("", ""));
    }
}
