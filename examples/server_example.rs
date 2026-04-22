use nwc_relay::{Config, open_whitelist_store, run_relay, run_http_server};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), String> {
    let config = Config::from_env();
    
    println!("Starting NWC Relay...");
    println!("  WebSocket port: {}", config.relay_port);
    println!("  HTTP port: {}", config.http_port);
    println!("  Data directory: {:?}", config.data_dir);
    
    let whitelist = open_whitelist_store(&config.data_dir)
        .map_err(|e| format!("Failed to open whitelist: {}", e))?;
    let whitelist = Arc::new(whitelist);
    
    let ws_handle = tokio::spawn(run_relay(config.clone(), whitelist.clone()));
    let http_handle = tokio::spawn(run_http_server(config.clone()));
    
    println!("Relay running!");
    
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
    
    Ok(())
}