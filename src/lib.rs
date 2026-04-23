#![allow(dead_code)]

pub mod cli;
pub mod config;
pub mod whitelist;
pub mod error;
pub mod nips;
pub mod relay;

#[allow(dead_code)]
pub use cli::{validate_pubkey, handle_add, handle_remove, handle_list};
pub use config::Config;
pub use whitelist::{WhitelistStore, open_whitelist_store};
pub use nips::nip_11::run_http_server;
pub use nips::nip_11::RelayInfo;
pub use relay::run_relay;