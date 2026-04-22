mod config;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
            println!("Adding pubkey: {} (storage not yet implemented)", pubkey);
        }
        Commands::Remove { pubkey } => {
            if let Err(e) = validate_pubkey(&pubkey) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            println!("Removing pubkey: {} (storage not yet implemented)", pubkey);
        }
        Commands::List => {
            println!("Listing pubkeys (storage not yet implemented)");
        }
        Commands::Run { port } => {
            let config = config::Config::from_env();
            let final_port = port.unwrap_or(config.relay_port);
            println!("Starting relay on port {}...", final_port);
        }
    }
}