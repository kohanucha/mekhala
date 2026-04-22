mod test_conf {
    use std::path::PathBuf;

    #[test]
    fn test_default_config() {
        let config = nwc_relay::conf::Config::default();
        
        assert_eq!(config.relay_port, 7777);
        assert_eq!(config.http_port, 7778);
        assert_eq!(config.relay_name, "NWC Relay");
        assert_eq!(config.relay_description, "Ultra-lite NWC relay for private home use");
        assert_eq!(config.data_dir, PathBuf::from("/data"));
    }

    #[test]
    fn test_default_port() {
        std::env::remove_var("RELAY_PORT");
        std::env::remove_var("HTTP_PORT");
        std::env::remove_var("RELAY_NAME");
        std::env::remove_var("RELAY_DESCRIPTION");
        std::env::remove_var("DATA_DIR");
        
        let config = nwc_relay::conf::Config::from_env();
        
        assert_eq!(config.relay_port, 7777);
        assert_eq!(config.http_port, 7778);
    }

    #[test]
    fn test_env_port_override() {
        std::env::set_var("RELAY_PORT", "9000");
        std::env::set_var("HTTP_PORT", "9001");
        
        let config = nwc_relay::conf::Config::from_env();
        
        assert_eq!(config.relay_port, 9000);
        assert_eq!(config.http_port, 9001);
        
        std::env::remove_var("RELAY_PORT");
        std::env::remove_var("HTTP_PORT");
    }

    #[test]
    fn test_env_name_override() {
        std::env::set_var("RELAY_NAME", "Test Relay");
        
        let config = nwc_relay::conf::Config::from_env();
        
        assert_eq!(config.relay_name, "Test Relay");
        
        std::env::remove_var("RELAY_NAME");
    }

    #[test]
    fn test_env_description_override() {
        std::env::set_var("RELAY_DESCRIPTION", "Custom description");
        
        let config = nwc_relay::conf::Config::from_env();
        
        assert_eq!(config.relay_description, "Custom description");
        
        std::env::remove_var("RELAY_DESCRIPTION");
    }

    #[test]
    fn test_env_data_dir_override() {
        std::env::set_var("DATA_DIR", "/custom/data");
        
        let config = nwc_relay::conf::Config::from_env();
        
        assert_eq!(config.data_dir, PathBuf::from("/custom/data"));
        
        std::env::remove_var("DATA_DIR");
    }

    #[test]
    fn test_config_clone() {
        let config = nwc_relay::conf::Config::default();
        let cloned = config.clone();
        
        assert_eq!(config.relay_port, cloned.relay_port);
        assert_eq!(config.http_port, cloned.http_port);
        assert_eq!(config.relay_name, cloned.relay_name);
    }
}