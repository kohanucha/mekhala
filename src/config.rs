use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub relay_port: u16,
    pub relay_name: String,
    pub relay_description: String,
    pub data_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            relay_port: 7777,
            relay_name: "NWC Relay".to_string(),
            relay_description: "Ultra-lite NWC relay for private home use".to_string(),
            data_dir: PathBuf::from("/data"),
        }
    }
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            relay_port: env::var("RELAY_PORT")
                .unwrap_or_else(|_| "7777".to_string())
                .parse()
                .unwrap_or(7777),
            relay_name: env::var("RELAY_NAME").unwrap_or_else(|_| "NWC Relay".to_string()),
            relay_description: env::var("RELAY_DESCRIPTION")
                .unwrap_or_else(|_| "Ultra-lite NWC relay for private home use".to_string()),
            data_dir: env::var("DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/data")),
        }
    }

    pub fn whitelist_path(&self) -> PathBuf {
        self.data_dir.join("whitelist.db")
    }
}