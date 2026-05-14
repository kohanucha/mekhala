pub mod transport;
pub mod hibernation;
pub mod durable_object;
pub mod connection;

pub use transport::{CloudflareTransport};
pub use hibernation::HibernationState;
pub use durable_object::get_durable_stub;

use worker::*;

pub fn apply_security_headers(response: Response) -> Result<Response> {
    let headers = response.headers().clone();
    headers.set("Strict-Transport-Security", "max-age=31536000; includeSubDomains")?;
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
