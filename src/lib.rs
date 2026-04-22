pub mod cli;
pub mod config;
pub mod whitelist;
pub mod error;
pub mod nips;

pub use cli::{Cli, Commands, validate_pubkey, handle_add, handle_remove, handle_list, handle_run};
pub use config::Config;
pub use whitelist::{WhitelistStore, open_whitelist_store};
pub use error::{Error, Result};
pub use nips::{run_ws_server, run_http_server};
pub use nips::nip_11::RelayInfo;