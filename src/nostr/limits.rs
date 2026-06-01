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
#[path = "limits_test.rs"]
mod limits_test;