mod config;
mod whitelist;
mod relay;
mod http;
mod relay_info;

use clap::{Parser, Subcommand};
use std::sync::Arc;

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

#[tokio::main]
async fn main() -> Result<(), String> {
    env_logger::init();
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    match cli.command {
        Commands::Add { pubkey } => {
            if let Err(e) = validate_pubkey(&pubkey) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            let config = config::Config::from_env();
            match whitelist::open_whitelist_store(&config.data_dir) {
                Ok(store) => {
                    let store = Arc::new(store);
                    let was_present = store.add(&pubkey).await?;
                    if was_present {
                        println!("Pubkey already exists: {}", pubkey);
                    } else {
                        println!("Added pubkey: {}", pubkey);
                    }
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
            let config = config::Config::from_env();
            match whitelist::open_whitelist_store(&config.data_dir) {
                Ok(store) => {
                    let store = Arc::new(store);
                    let was_present = store.remove(&pubkey).await?;
                    if was_present {
                        println!("Removed pubkey: {}", pubkey);
                    } else {
                        println!("Pubkey not found: {}", pubkey);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to open whitelist store: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::List => {
            let config = config::Config::from_env();
            match whitelist::open_whitelist_store(&config.data_dir) {
                Ok(store) => {
                    let store = Arc::new(store);
                    let pubkeys = store.list().await?;
                    if pubkeys.is_empty() {
                        println!("No whitelisted pubkeys");
                    } else {
                        for pk in pubkeys {
                            println!("{}", pk);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to open whitelist store: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Run { port } => {
            let config = config::Config::from_env();
            let final_port = port.unwrap_or(config.relay_port);
            let run_config = config::Config {
                relay_port: final_port,
                http_port: config.http_port,
                relay_name: config.relay_name,
                relay_description: config.relay_description,
                data_dir: config.data_dir,
            };

            println!("Starting WebSocket relay on port {}...", final_port);
            println!("Starting HTTP relay on port {}...", config.http_port);
            println!("Data directory: {:?}", run_config.data_dir);

            let whitelist_store = whitelist::open_whitelist_store(&run_config.data_dir)
                .expect("Failed to open whitelist store");
            let whitelist = Arc::new(whitelist_store);

            let ws_handle = tokio::spawn(relay::run_server(run_config.clone(), whitelist.clone()));
            let http_handle = tokio::spawn(http::run_http_server(run_config.clone()));

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
    }
    Ok(())
}