mod cli;
mod config;
mod whitelist;
mod error;
mod nips;

use clap::{Parser, Subcommand};
use std::sync::Arc;

use crate::config::Config;
use crate::whitelist::open_whitelist_store;
use crate::cli::{validate_pubkey, handle_add, handle_remove, handle_list};

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