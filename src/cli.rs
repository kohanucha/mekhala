use clap::{Parser, Subcommand};
use std::sync::Arc;

use crate::config::Config;
use crate::whitelist::WhitelistStore;
use crate::relay;

#[derive(Parser)]
#[command(name = "nwc-relay")]
#[command(about = "Ultra-lite NWC Relay for private home use")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Add {
        #[arg(help = "Hex-encoded pubkey to whitelist")]
        pubkey: String,
    },
    Remove {
        #[arg(help = "Hex-encoded pubkey to remove")]
        pubkey: String,
    },
    List,
    Run {
        #[arg(long, help = "Override relay port")]
        port: Option<u16>,
    },
}

pub fn validate_pubkey(pubkey: &str) -> Result<(), String> {
    if pubkey.len() != 64 {
        return Err("Pubkey must be 64 hex characters".to_string());
    }
    if pubkey.chars().any(|c| !c.is_ascii_hexdigit()) {
        return Err("Pubkey must be valid hex".to_string());
    }
    Ok(())
}

pub async fn handle_add(store: Arc<WhitelistStore>, pubkey: &str) -> Result<String, String> {
    let was_present = store.add(pubkey).await?;
    if was_present {
        Ok(format!("Pubkey already exists: {}", pubkey))
    } else {
        Ok(format!("Added pubkey: {}", pubkey))
    }
}

pub async fn handle_remove(store: Arc<WhitelistStore>, pubkey: &str) -> Result<String, String> {
    let was_present = store.remove(pubkey).await?;
    if was_present {
        Ok(format!("Removed pubkey: {}", pubkey))
    } else {
        Ok(format!("Pubkey not found: {}", pubkey))
    }
}

pub async fn handle_list(store: Arc<WhitelistStore>) -> Result<String, String> {
    let pubkeys = store.list().await?;
    if pubkeys.is_empty() {
        Ok("No whitelisted pubkeys".to_string())
    } else {
        Ok(pubkeys.join("\n"))
    }
}

pub async fn handle_run(config: Config, whitelist: Arc<WhitelistStore>) {
    println!("Starting NWC Relay on port {}...", config.relay_port);
    println!("Starting HTTP relay info on port {}...", config.http_port);
    println!("Data directory: {:?}", config.data_dir);

    let ws_handle = tokio::spawn(relay::run_relay(config.clone(), whitelist.clone()));
    let http_handle = tokio::spawn(crate::nips::run_http_server(config.clone()));

    tokio::select! {
        result = ws_handle => {
            if let Err(e) = result {
                eprintln!("WebSocket server error: {}", e);
            }
        }
        result = http_handle => {
            if let Err(e) = result {
                eprintln!("HTTP server error: {}", e);
            }
        }
    }
}