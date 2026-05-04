use worker::*;

mod limits;
mod error;
mod auth;
mod server;
mod cloudflare;
mod nostr;
mod lnaddress;
mod util;

pub use limits::Limits;
pub use error::RelayError;

use server::Server;

pub use cloudflare::Websocket;

pub fn set_panic_hook() {
    console_error_panic_hook::set_once();
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    set_panic_hook();
    Server::run(req, env).await
}