use std::sync::Arc;
use clap::{Parser, Subcommand};

use nwc_relay::{Config, open_whitelist_store, WhitelistStore};

#[derive(Parser)]
#[command(name = "nwc-relay-cli")]
#[command(about = "Manage whitelist for NWC Relay")]
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
}

fn validate_pubkey(pubkey: &str) -> Result<(), String> {
    if pubkey.len() != 64 {
        return Err("Pubkey must be 64 hex characters".to_string());
    }
    if pubkey.chars().any(|c| !c.is_ascii_hexdigit()) {
        return Err("Pubkey must be valid hex".to_string());
    }
    Ok(())
}

async fn handle_add(store: Arc<WhitelistStore>, pubkey: &str) -> Result<String, String> {
    let was_present = store.add(pubkey).await?;
    if was_present {
        Ok(format!("Pubkey already exists: {}", pubkey))
    } else {
        Ok(format!("Added pubkey: {}", pubkey))
    }
}

async fn handle_remove(store: Arc<WhitelistStore>, pubkey: &str) -> Result<String, String> {
    let was_present = store.remove(pubkey).await?;
    if was_present {
        Ok(format!("Removed pubkey: {}", pubkey))
    } else {
        Ok(format!("Pubkey not found: {}", pubkey))
    }
}

async fn handle_list(store: Arc<WhitelistStore>) -> Result<String, String> {
    let pubkeys = store.list().await?;
    if pubkeys.is_empty() {
        Ok("No whitelisted pubkeys".to_string())
    } else {
        Ok(pubkeys.join("\n"))
    }
}

#[tokio::main]
async fn main() -> Result<(), String> {
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    match cli.command {
        Commands::Add { pubkey } => {
            if let Err(e) = validate_pubkey(&pubkey) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            let config = Config::from_env();
            match open_whitelist_store(&config.data_dir) {
                Ok(store) => {
                    let store = Arc::new(store);
                    let result = handle_add(store, &pubkey).await?;
                    println!("{}", result);
                }
                Err(e) => {
                    eprintln!("Failed to open whitelist store: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Remove { pubkey } => {
            if let Err(e) = validate_pubkey(&pubkey) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            let config = Config::from_env();
            match open_whitelist_store(&config.data_dir) {
                Ok(store) => {
                    let store = Arc::new(store);
                    let result = handle_remove(store, &pubkey).await?;
                    println!("{}", result);
                }
                Err(e) => {
                    eprintln!("Failed to open whitelist store: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::List => {
            let config = Config::from_env();
            match open_whitelist_store(&config.data_dir) {
                Ok(store) => {
                    let store = Arc::new(store);
                    let result = handle_list(store).await?;
                    println!("{}", result);
                }
                Err(e) => {
                    eprintln!("Failed to open whitelist store: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
    Ok(())
}