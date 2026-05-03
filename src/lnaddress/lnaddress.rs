use sha2::{Sha256, Digest};
use serde_json::Value;

pub struct LNAddress<'a> {
    pub username: &'a str,
}

impl<'a> LNAddress<'a> {
    pub fn new(username: &'a str) -> Self {
        Self { username }
    }

    pub fn generate_metadata(&self) -> String {
        format!("[[\"text/plain\",\"Payment to {}\"]]", self.username)
    }

    pub fn get_info(&self, callback_url: &str) -> Value {
        serde_json::json!({
            "callback": callback_url,
            "maxSendable": 100000000,
            "minSendable": 1000,
            "metadata": self.generate_metadata(),
            "tag": "payRequest"
        })
    }

    pub fn get_description_hash(&self) -> String {
        let metadata = self.generate_metadata();
        let mut hasher = Sha256::new();
        hasher.update(metadata.as_bytes());
        let hash = hasher.finalize();
        hex::encode(hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_creates_instance() {
        let addr = LNAddress::new("user");
        assert_eq!(addr.username, "user");
    }

    #[test]
    fn test_generate_metadata_format() {
        let addr = LNAddress::new("alice");
        let metadata = addr.generate_metadata();
        assert_eq!(metadata, "[[\"text/plain\",\"Payment to alice\"]]");
    }

    #[test]
    fn test_generate_metadata_special_characters() {
        let addr = LNAddress::new("user@test");
        let metadata = addr.generate_metadata();
        assert!(metadata.contains("user@test"));
    }

    #[test]
    fn test_generate_metadata_empty_username() {
        let addr = LNAddress::new("");
        let metadata = addr.generate_metadata();
        assert_eq!(metadata, "[[\"text/plain\",\"Payment to \"]]");
    }

    #[test]
    fn test_description_hash_length() {
        let addr = LNAddress::new("testuser");
        let hash = addr.get_description_hash();
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_description_hash_deterministic() {
        let addr = LNAddress::new("testuser");
        let hash1 = addr.get_description_hash();
        let hash2 = addr.get_description_hash();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_description_hash_different_users() {
        let addr1 = LNAddress::new("user1");
        let addr2 = LNAddress::new("user2");
        let hash1 = addr1.get_description_hash();
        let hash2 = addr2.get_description_hash();
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_get_info_all_fields() {
        let addr = LNAddress::new("testuser");
        let info = addr.get_info("https://callback.url");
        assert!(info.get("callback").is_some());
        assert!(info.get("maxSendable").is_some());
        assert!(info.get("minSendable").is_some());
        assert!(info.get("metadata").is_some());
        assert!(info.get("tag").is_some());
    }

    #[test]
    fn test_get_info_callback_url() {
        let addr = LNAddress::new("testuser");
        let info = addr.get_info("https://custom.callback/123");
        assert_eq!(info["callback"], "https://custom.callback/123");
    }

    #[test]
    fn test_get_info_max_sendable() {
        let addr = LNAddress::new("testuser");
        let info = addr.get_info("https://callback.url");
        assert_eq!(info["maxSendable"], 100000000);
    }

    #[test]
    fn test_get_info_min_sendable() {
        let addr = LNAddress::new("testuser");
        let info = addr.get_info("https://callback.url");
        assert_eq!(info["minSendable"], 1000);
    }

    #[test]
    fn test_get_info_tag() {
        let addr = LNAddress::new("testuser");
        let info = addr.get_info("https://callback.url");
        assert_eq!(info["tag"], "payRequest");
    }

    #[test]
    fn test_get_info_metadata_contains_username() {
        let addr = LNAddress::new("alice");
        let info = addr.get_info("https://callback.url");
        assert!(info["metadata"].as_str().unwrap().contains("alice"));
    }
}