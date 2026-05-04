use crate::auth::Authenticator;
use crate::cloudflare::{apply_security_headers, create_cors_response};
use crate::lnaddress::{handle_lnaddress, handle_lnaddress_callback};
use crate::nostr;
use worker::*;

pub struct Server;

impl Server {
    pub async fn run(req: Request, env: Env) -> Result<Response> {
        let router = Router::new();

        router
            .get_async("/", Self::handle_request)
            .get_async("/:secret", Self::handle_request)
            .get_async("/.well-known/lnurlp/:username", handle_lnaddress)
            .get_async("/lnaddress/:username/callback", handle_lnaddress_callback)
            .run(req, env)
            .await
    }

    async fn handle_request(req: Request, ctx: RouteContext<()>) -> Result<Response> {
        if req.method() == Method::Options {
            return create_cors_response(Response::ok("")?);
        }

        let auth = Authenticator::from_env(&ctx.env);
        let provided_secret = ctx.param("secret").map(|s| s.as_str()).unwrap_or_default();

        if !auth.is_authorized(provided_secret) {
            return apply_security_headers(Response::error("Not Found", 404)?);
        }

        if let Ok(Some(upgrade)) = req.headers().get("Upgrade") {
            if upgrade.to_lowercase() == "websocket" {
                return nostr::handle_nwc_websocket_upgrade(req, &ctx.env).await;
            }
        }

        nostr::handle_get_info()
    }
}
