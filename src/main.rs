#![allow(dead_code)]

mod cli;
mod config;
mod whitelist;
mod relay;

use std::sync::Arc;
use tokio::signal;

use nwc_relay::{Config, open_whitelist_store, run_relay, run_http_server};

#[tokio::main]
async fn main() -> Result<(), String> {
    env_logger::init();
    dotenvy::dotenv().ok();

    let config = Config::from_env();

    println!("Starting NWC Relay...");
    println!("  WebSocket: 0.0.0.0:{}", config.relay_port);
    println!("  HTTP:      0.0.0.0:{}", config.http_port);
    println!("  Data dir:  {:?}", config.data_dir);
    println!();

    let whitelist_store = open_whitelist_store(&config.data_dir)
        .map_err(|e| format!("Failed to open whitelist store: {}", e))?;
    let whitelist = Arc::new(whitelist_store);

    let ws_handle = tokio::spawn(run_relay(config.clone(), whitelist.clone()));
    let http_handle = tokio::spawn(run_http_server(config.clone()));

    println!("NWC Relay is running.");
    println!("Press Ctrl+C to stop.");

    tokio::select! {
        _ = signal::ctrl_c() => {
            println!("\nShutting down...");
        }
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

    println!("NWC Relay stopped.");
    Ok(())
}