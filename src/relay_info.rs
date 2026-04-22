use serde::Serialize;
use crate::config::Config;

#[derive(Debug, Clone, Serialize)]
pub struct RelayInfo {
    pub name: String,
    pub description: String,
    pub pubkey: String,
    pub contact: String,
    pub supported_nips: Vec<u32>,
    pub software: String,
    pub version: String,
}

impl RelayInfo {
    pub fn from_config(config: &Config) -> Self {
        Self {
            name: config.relay_name.clone(),
            description: config.relay_description.clone(),
            pubkey: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            contact: String::new(),
            supported_nips: vec![1, 11, 47],
            software: "https://github.com/rust-nwc/nwc-relay".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}