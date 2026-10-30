//! Mekhala — Nostr Wallet Connect (NIP-47) relay for Cloudflare Workers.
//!
//! The Worker upgrades WebSocket connections to a Durable Object that runs
//! the relay: it validates NIP-01 events, keeps subscription and
//! wallet-service info registries backed by durable storage, and routes
//! NIP-47 request/response events between clients and wallet services.
//!
//! Module layout:
//! - `cloudflare`: Worker entry, router, transport (Durable Object), config
//! - `nostr`: NIP protocol types (messages, events, filters, tags),
//!   encryption (NIP-04, NIP-44), the NIP-47 request/response machinery,
//!   and the subscription/KV registry
//! - `lnaddress`: Lightning Address resolution and wallet sessions
//! - `auth`, `common`, `util`: shared helpers

use worker::*;

mod auth;
mod common;
mod cloudflare;
mod nostr;
mod lnaddress;
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
/// request to the router (LNURL HTTP routes and WebSocket upgrade).
#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    set_panic_hook();
    cloudflare::router::run(req, env).await
}