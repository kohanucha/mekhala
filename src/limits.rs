use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Limits {
    pub max_filter_items: usize,
    pub max_event_tags: usize,
    pub max_content_length: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_filter_items: 100,
            max_event_tags: 100,
            max_content_length: 32768,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_limits_default() {
        let limits = Limits::default();
        assert_eq!(limits.max_filter_items, 100);
        assert_eq!(limits.max_event_tags, 100);
        assert_eq!(limits.max_content_length, 32768);
    }

    #[test]
    fn test_limits_values() {
        let limits = Limits {
            max_filter_items: 50,
            max_event_tags: 25,
            max_content_length: 10000,
        };
        assert_eq!(limits.max_filter_items, 50);
        assert_eq!(limits.max_event_tags, 25);
        assert_eq!(limits.max_content_length, 10000);
    }
}