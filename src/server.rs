use crate::auth::{AccessPolicy, AuthError};
use crate::cloudflare::{apply_security_headers, create_cors_response, CloudflareKvStore};
use crate::lnaddress::LnAddressHandler;
use worker::*;

pub async fn run(req: Request, env: Env) -> Result<Response> {
    let router = Router::new();

    router
        .get_async("/", handle_request)
        .get_async("/:secret", handle_request)
        .get_async("/.well-known/lnurlp/:username", |req, ctx| async move {
            let kv = ctx.env.kv("MEKHALA_NWC_KV")?;
            let kv_store = CloudflareKvStore::new(kv);
            let handler = LnAddressHandler::new(&kv_store);
            let username = ctx.param("username").ok_or_else(|| Error::from("Missing username"))?;
            if !crate::lnaddress::is_valid_username(&username) {
                return create_cors_response(Response::from_json(&serde_json::json!({ "status": "ERROR", "reason": "Not Found" }))?.with_status(404));
            }
            match handler.handle_pay_request(req, &username).await {
                Ok(resp) => Ok(resp),
                Err(e) => {
                    let error_body = serde_json::json!({ "status": "ERROR", "reason": e.to_string() });
                    create_cors_response(Response::from_json(&error_body)?.with_status(200))
                }
            }
        })
        .get_async("/lnaddress/:username/callback", |req, ctx| async move {
            crate::cloudflare::transport::connect(req, &ctx.env).await
        })
        .run(req, env)
        .await
}

async fn handle_request(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if req.method() == Method::Options {
        return handle_options();
    }

    if let Some(auth_error) = handle_auth(&req, &ctx)? {
        return Ok(auth_error);
    }

    if is_websocket_upgrade(&req) {
        return handle_upgrade(req, &ctx).await;
    }

    handle_nip11()
}

fn handle_options() -> Result<Response> {
    create_cors_response(Response::ok("")?)
}

fn handle_auth(_req: &Request, ctx: &RouteContext<()>) -> Result<Option<Response>> {
    let policy = AccessPolicy::from_env(&ctx.env);
    let provided_secret = ctx.param("secret").map(|s| s.as_str()).unwrap_or_default();

    match policy.check_access(provided_secret) {
        Err(AuthError::Forbidden) => {
            Ok(Some(apply_security_headers(Response::error("Not Found", 404)?)?))
        }
        Ok(_) => Ok(None),
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
    let info = serde_json::json!({"supported_nips": [1, 9, 11, 47]});
    crate::cloudflare::transport::create_response(info, "application/nostr+json")
}