mod test_nip_11 {
    use std::path::PathBuf;
    use nwc_relay::conf::Config;
    use nwc_relay::nips::nip_11::RelayInfo;

    #[test]
    fn test_relay_info_from_config() {
        let config = Config {
            relay_port: 7777,
            http_port: 7778,
            relay_name: "Test Relay".to_string(),
            relay_description: "Test description".to_string(),
            data_dir: PathBuf::from("/data"),
        };
        
        let relay_info = RelayInfo::from_config(&config);
        
        assert_eq!(relay_info.name, "Test Relay");
        assert_eq!(relay_info.description, "Test description");
    }

    #[test]
    fn test_relay_info_default_pubkey() {
        let config = Config::default();
        let relay_info = RelayInfo::from_config(&config);
        
        assert_eq!(relay_info.pubkey, "0000000000000000000000000000000000000000000000000000000000000000");
    }

    #[test]
    fn test_relay_info_to_json() {
        let config = Config::default();
        let relay_info = RelayInfo::from_config(&config);
        
        let json = relay_info.to_json();
        
        assert!(json.contains("\"name\""));
        assert!(json.contains("\"description\""));
        assert!(json.contains("\"supported_nips\""));
    }

    #[test]
    fn test_supported_nips() {
        let config = Config::default();
        let relay_info = RelayInfo::from_config(&config);
        
        assert_eq!(relay_info.supported_nips, vec![1, 11, 47]);
    }

    #[test]
    fn test_supported_nips_contains_all() {
        let config = Config::default();
        let relay_info = RelayInfo::from_config(&config);
        
        assert!(relay_info.supported_nips.contains(&1));
        assert!(relay_info.supported_nips.contains(&11));
        assert!(relay_info.supported_nips.contains(&47));
    }

    #[test]
    fn test_relay_info_software() {
        let config = Config::default();
        let relay_info = RelayInfo::from_config(&config);
        
        assert!(relay_info.software.contains("github.com"));
    }

    #[test]
    fn test_relay_info_version() {
        let config = Config::default();
        let relay_info = RelayInfo::from_config(&config);
        
        assert!(!relay_info.version.is_empty());
    }

    #[test]
    fn test_relay_info_json_deserialization() {
        let config = Config::default();
        let relay_info = RelayInfo::from_config(&config);
        let json = relay_info.to_json();
        
        let deserialized: RelayInfo = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.name, relay_info.name);
        assert_eq!(deserialized.supported_nips, relay_info.supported_nips);
    }

    #[test]
    fn test_relay_info_empty_contact() {
        let config = Config::default();
        let relay_info = RelayInfo::from_config(&config);
        
        assert!(relay_info.contact.is_empty());
    }
}