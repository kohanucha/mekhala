mod config;
mod whitelist;

use clap::{Parser, Subcommand};

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

fn main() {
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
                    let store = std::sync::Arc::new(store);
                    let runtime = tokio::runtime::Runtime::new().unwrap();
                    let was_present = runtime.block_on(store.add(&pubkey)).unwrap();
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
                    let store = std::sync::Arc::new(store);
                    let runtime = tokio::runtime::Runtime::new().unwrap();
                    let was_present = runtime.block_on(store.remove(&pubkey)).unwrap();
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
                    let store = std::sync::Arc::new(store);
                    let runtime = tokio::runtime::Runtime::new().unwrap();
                    let pubkeys = runtime.block_on(store.list()).unwrap();
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
            println!("Starting relay on port {}...", final_port);
            println!("Data directory: {:?}", config.data_dir);
        }
    }
}