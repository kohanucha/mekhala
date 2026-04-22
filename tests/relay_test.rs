use nwc_relay::{Config, open_whitelist_store, RelayInfo};
use std::path::PathBuf;
use tempfile::TempDir;

mod config_tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        
        assert_eq!(config.relay_port, 7777);
        assert_eq!(config.http_port, 7778);
        assert_eq!(config.relay_name, "NWC Relay");
        assert_eq!(config.relay_description, "Ultra-lite NWC relay for private home use");
    }

    #[test]
    fn test_config_clone() {
        let config = Config::default();
        let cloned = config.clone();
        
        assert_eq!(config.relay_port, cloned.relay_port);
        assert_eq!(config.relay_name, cloned.relay_name);
    }

    #[test]
    fn test_env_port_override() {
        std::env::set_var("RELAY_PORT", "9000");
        std::env::set_var("HTTP_PORT", "9001");
        
        let config = Config::from_env();
        
        assert_eq!(config.relay_port, 9000);
        assert_eq!(config.http_port, 9001);
        
        std::env::remove_var("RELAY_PORT");
        std::env::remove_var("HTTP_PORT");
    }

    #[test]
    fn test_env_name_override() {
        std::env::set_var("RELAY_NAME", "Test Relay");
        
        let config = Config::from_env();
        
        assert_eq!(config.relay_name, "Test Relay");
        
        std::env::remove_var("RELAY_NAME");
    }

    #[test]
    fn test_env_data_dir_override() {
        std::env::set_var("DATA_DIR", "/custom/data");
        
        let config = Config::from_env();
        
        assert_eq!(config.data_dir, PathBuf::from("/custom/data"));
        
        std::env::remove_var("DATA_DIR");
    }
}

mod db_tests {
    use super::*;

    #[test]
    fn test_open_whitelist_store() {
        let temp_dir = TempDir::new().unwrap();
        let store = open_whitelist_store(&temp_dir.path().to_path_buf());
        assert!(store.is_ok());
    }

    #[tokio::test]
    async fn test_whitelist_add() {
        let temp_dir = TempDir::new().unwrap();
        let store = open_whitelist_store(&temp_dir.path().to_path_buf()).unwrap();
        
        let pubkey = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        store.add(pubkey).await.unwrap();
        
        let contains = store.contains(pubkey).await.unwrap();
        assert!(contains);
    }

    #[tokio::test]
    async fn test_whitelist_list() {
        let temp_dir = TempDir::new().unwrap();
        let store = open_whitelist_store(&temp_dir.path().to_path_buf()).unwrap();
        
        store.add("aaa").await.unwrap();
        store.add("bbb").await.unwrap();
        
        let pubkeys = store.list().await.unwrap();
        assert_eq!(pubkeys.len(), 2);
    }

    #[tokio::test]
    async fn test_whitelist_remove() {
        let temp_dir = TempDir::new().unwrap();
        let store = open_whitelist_store(&temp_dir.path().to_path_buf()).unwrap();
        
        let pubkey = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        store.add(pubkey).await.unwrap();
        store.remove(pubkey).await.unwrap();
        
        let contains = store.contains(pubkey).await.unwrap();
        assert!(!contains);
    }
}

mod nip_11_tests {
    use super::*;

    #[test]
    fn test_relay_info_from_config() {
        let config = Config {
            relay_port: 7777,
            http_port: 7778,
            relay_name: "Test".to_string(),
            relay_description: "Desc".to_string(),
            data_dir: PathBuf::from("/data"),
        };
        
        let relay_info = RelayInfo::from_config(&config);
        
        assert_eq!(relay_info.name, "Test");
        assert_eq!(relay_info.description, "Desc");
    }

    #[test]
    fn test_supported_nips() {
        let config = Config::default();
        let relay_info = RelayInfo::from_config(&config);
        
        assert_eq!(relay_info.supported_nips, vec![1, 11, 47]);
    }

    #[test]
    fn test_json_roundtrip() {
        let config = Config::default();
        let relay_info = RelayInfo::from_config(&config);
        let json = relay_info.to_json();
        
        let deserialized: RelayInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, relay_info.name);
        assert_eq!(deserialized.supported_nips, relay_info.supported_nips);
    }
}