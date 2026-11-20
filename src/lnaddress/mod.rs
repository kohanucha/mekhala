//! Lightning Address (LNURL) support: serves `payRequest` metadata for
//! local usernames, validates requests, and mints invoices by driving a
//! NIP-47 session with the user's connected wallet.

pub(crate) mod gateway;
pub(crate) mod policy;
pub(crate) mod rate_limit;
pub(crate) mod record;
pub(crate) mod registration;
mod validation;
pub(crate) mod wallet_connector;

pub(crate) use policy::LnAddressPolicy;
pub(crate) use record::LnAddressKeys;
pub(crate) use validation::is_valid_username;
