//! Lightning Address (LNURL) support: serves `payRequest` metadata for
//! local usernames, validates requests, and mints invoices by driving a
//! NIP-47 session with the user's connected wallet.

pub(crate) mod gateway;
mod validation;
mod wallet_connector;

pub(crate) use validation::is_valid_username;
