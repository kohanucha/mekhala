use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Limits {
    pub max_content_length: usize,
}

impl Limits {
    pub fn new(max_content_length: usize) -> Self {
        Self {
            max_content_length,
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_content_length: 65536,
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
    }
}