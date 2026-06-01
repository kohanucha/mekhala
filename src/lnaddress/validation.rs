pub(crate) fn is_valid_username(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '~')
}

#[cfg(test)]
#[path = "validation_test.rs"]
mod validation_test;
