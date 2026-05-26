pub(crate) fn is_valid_username(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '~')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_alphanumeric() {
        assert!(is_valid_username("alice"));
    }

    #[test]
    fn test_valid_with_underscore() {
        assert!(is_valid_username("alice_smith"));
    }

    #[test]
    fn test_valid_with_hyphen() {
        assert!(is_valid_username("alice-smith"));
    }

    #[test]
    fn test_valid_numeric() {
        assert!(is_valid_username("user123"));
    }

    #[test]
    fn test_valid_single_char() {
        assert!(is_valid_username("a"));
    }

    #[test]
    fn test_invalid_empty() {
        assert!(!is_valid_username(""));
    }

    #[test]
    fn test_invalid_special_chars() {
        assert!(!is_valid_username("alice@smith"));
    }

    #[test]
    fn test_invalid_space() {
        assert!(!is_valid_username("alice smith"));
    }

    #[test]
    fn test_valid_dot() {
        assert!(is_valid_username("alice.smith"));
    }

    #[test]
    fn test_valid_tilde() {
        assert!(is_valid_username("alice~smith"));
    }

    #[test]
    fn test_invalid_unicode() {
        assert!(!is_valid_username("hëllo"));
    }

    #[test]
    fn test_invalid_unicode_emoji() {
        assert!(!is_valid_username("alice😀"));
    }
}
