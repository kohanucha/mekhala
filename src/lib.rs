//! Mekhala — Nostr Wallet Connect (NIP-47) relay for Cloudflare Workers.
//!
//! The Worker upgrades WebSocket connections to a Durable Object that runs
//! the relay: it validates NIP-01 events, keeps subscription and
//! wallet-service info registries backed by durable storage, and routes
//! NIP-47 request/response events between clients and wallet services.
//!
//! The relay only validates and routes: it never decrypts NIP-47 content and
//! holds no wallet credentials.
//!
//! Module layout:
//! - `cloudflare`: Worker entry, router, transport (Durable Object), config
//! - `nostr`: NIP-01 protocol types (messages, events, filters, tags), the
//!   relay engine and the subscription registry
//! - `auth`, `util`: relay-secret check and shared helpers
//! - `common` (tests only): shared test fixtures

use worker::{event, Context, Env, Request, Response, Result};

mod auth;
mod cloudflare;
#[cfg(test)]
mod common;
mod nostr;
mod util;

pub use cloudflare::CloudflareTransport;

/// Installs a panic hook that routes panics to `console.error` for
/// Cloudflare's log tail.
///
/// The release profile builds with `panic = "abort"`, so a panic terminates
/// the Worker isolate regardless; this hook only makes the message visible
/// in the platform logs. Call once at Worker startup.
pub fn set_panic_hook() {
    console_error_panic_hook::set_once();
}

/// Worker fetch handler: installs the panic hook, then delegates every
/// request to the router (NIP-11 document and WebSocket upgrade).
///
/// # Errors
///
/// Propagates router failures (unroutable requests, failed WebSocket
/// upgrades, or errors building a response).
#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    set_panic_hook();
    cloudflare::router::run(req, env).await
}
