use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Limits {
    pub max_filter_items: usize,
    pub max_event_tags: usize,
    pub max_content_length: usize,
}

impl Limits {
    pub fn new(max_filter_items: usize, max_event_tags: usize, max_content_length: usize) -> Self {
        Self {
            max_filter_items,
            max_event_tags,
            max_content_length,
        }
    }
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
}