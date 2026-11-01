//! Cloudflare Worker bindings: transport, durable object, KV, routing.

pub mod auth;
pub mod config;
pub mod connection;
pub mod durable_object;
pub mod handler;
pub mod kv;
pub mod router;
pub mod transport;

pub use durable_object::get_durable_stub;
pub use kv::CloudflareKvStore;
pub use transport::CloudflareTransport;

use worker::{Error, Response, Result};

impl From<crate::common::NwcError> for Error {
    fn from(e: crate::common::NwcError) -> Self {
        Error::from(e.to_string())
    }
}

pub fn apply_security_headers(response: Response) -> Result<Response> {
    let headers = response.headers().clone();
    headers.set(
        "Strict-Transport-Security",
        "max-age=31536000; includeSubDomains",
    )?;
    headers.set("X-Content-Type-Options", "nosniff")?;
    headers.set("Content-Security-Policy", "default-src 'self'")?;
    Ok(response.with_headers(headers))
}

pub fn create_cors_response(response: Response) -> Result<Response> {
    let headers = response.headers().clone();
    headers.set("Access-Control-Allow-Origin", "*")?;
    headers.set("Access-Control-Allow-Methods", "GET, OPTIONS")?;
    headers.set("Access-Control-Allow-Headers", "*")?;

    apply_security_headers(response.with_headers(headers))
}
