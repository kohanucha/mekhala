use crate::auth::Authenticator;
use crate::cloudflare::{apply_security_headers, create_cors_response};
use crate::lnaddress::handle_lnaddress;
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
            .get_async("/lnaddress/:username/callback", |req, ctx| async move {
                crate::cloudflare::transport::connect(req, &ctx.env).await
            })
            .run(req, env)
            .await
    }

    async fn handle_request(req: Request, ctx: RouteContext<()>) -> Result<Response> {
        if req.method() == Method::Options {
            return Self::handle_options();
        }

        if let Some(auth_error) = Self::handle_auth(&req, &ctx)? {
            return Ok(auth_error);
        }

        if Self::is_websocket_upgrade(&req) {
            return Self::handle_upgrade(req, &ctx).await;
        }

        Self::handle_nip11()
    }

    fn handle_options() -> Result<Response> {
        create_cors_response(Response::ok("")?)
    }

    fn handle_auth(_req: &Request, ctx: &RouteContext<()>) -> Result<Option<Response>> {
        let auth = Authenticator::from_env(&ctx.env);
        let provided_secret = ctx.param("secret").map(|s| s.as_str()).unwrap_or_default();

        if !auth.is_authorized(provided_secret) {
            Ok(Some(apply_security_headers(Response::error("Not Found", 404)?)?))
        } else {
            Ok(None)
        }
    }

    fn is_websocket_upgrade(req: &Request) -> bool {
        req.headers()
            .get("Upgrade")
            .ok()
            .flatten()
            .map(|u| u.to_lowercase() == "websocket")
            .unwrap_or(false)
    }

    async fn handle_upgrade(req: Request, ctx: &RouteContext<()>) -> Result<Response> {
        crate::cloudflare::transport::connect(req, &ctx.env).await
    }

    fn handle_nip11() -> Result<Response> {
        let info = nostr::get_nip_11_info();
        crate::cloudflare::transport::create_response(info, "application/nostr+json")
    }
}
