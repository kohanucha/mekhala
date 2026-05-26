use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Limits {
    pub max_content_length: usize,
    pub max_subscriptions_per_connection: usize,
}

impl Limits {
    pub fn new(max_content_length: usize, max_subscriptions_per_connection: usize) -> Self {
        Self {
            max_content_length,
            max_subscriptions_per_connection,
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_content_length: 65536,
            max_subscriptions_per_connection: 100,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_limits_default() {
        let limits = Limits::default();
        assert_eq!(limits.max_content_length, 65536);
        assert_eq!(limits.max_subscriptions_per_connection, 100);
    }

    #[test]
    fn test_limits_new() {
        let limits = Limits::new(16384, 50);
        assert_eq!(limits.max_content_length, 16384);
        assert_eq!(limits.max_subscriptions_per_connection, 50);
    }
}