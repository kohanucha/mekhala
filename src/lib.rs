use worker::*;

mod auth;
mod server;
mod common;
mod cloudflare;
mod nostr;
mod lnaddress;
mod util;

pub use cloudflare::CloudflareTransport;

pub fn set_panic_hook() {
    console_error_panic_hook::set_once();
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    set_panic_hook();
    server::run(req, env).await
}