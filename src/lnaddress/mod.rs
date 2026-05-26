//! Lightning Address resolution and NWC session management.

pub(crate) mod gateway;
mod wallet_connector;
mod validation;

pub(crate) use validation::is_valid_username;
