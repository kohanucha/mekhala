use nwc_relay::{Config, open_whitelist_store, Cli, Commands};
use clap::Parser;

fn main() {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Add { pubkey } => {
            println!("Adding pubkey: {}", pubkey);
        }
        Commands::Remove { pubkey } => {
            println!("Removing pubkey: {}", pubkey);
        }
        Commands::List => {
            println!("Listing whitelisted pubkeys");
        }
        Commands::Run { port } => {
            println!("Running relay on port: {}", port.unwrap_or(7777));
        }
    }
}