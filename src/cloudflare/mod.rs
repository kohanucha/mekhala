//! Cloudflare Worker bindings: transport, durable object, KV, routing.

pub mod auth;
pub mod config;
pub mod connection;
pub mod durable_object;
pub mod identity;
pub mod router;
pub mod transport;

pub use durable_object::get_durable_stub;
pub use transport::CloudflareTransport;

use worker::{Response, Result};

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
