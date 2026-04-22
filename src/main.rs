mod conf;
mod db;
mod cli;
mod nips;

use std::sync::Arc;
use clap::Parser;
use crate::cli::{Cli, Commands, validate_pubkey, handle_add, handle_remove, handle_list, handle_run};

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
            let config = conf::Config::from_env();
            match db::open_whitelist_store(&config.data_dir) {
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
            let config = conf::Config::from_env();
            match db::open_whitelist_store(&config.data_dir) {
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
            let config = conf::Config::from_env();
            match db::open_whitelist_store(&config.data_dir) {
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
        Commands::Run { port } => {
            let config = conf::Config::from_env();
            let final_port = port.unwrap_or(config.relay_port);
            let run_config = conf::Config {
                relay_port: final_port,
                ..config
            };

            let whitelist_store = db::open_whitelist_store(&run_config.data_dir)
                .expect("Failed to open whitelist store");
            let whitelist = Arc::new(whitelist_store);

            handle_run(run_config, whitelist).await;
        }
    }
    Ok(())
}