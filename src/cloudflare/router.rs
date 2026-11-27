use crate::auth::AuthError;
use crate::cloudflare::auth;
use crate::cloudflare::config::CloudflareConfig;
use crate::cloudflare::{apply_security_headers, create_cors_response};
use crate::log_error;
use worker::{Env, Method, Request, Response, Result, RouteContext, Router};

pub async fn run(req: Request, env: Env) -> Result<Response> {
    let router = Router::new();

    router
        .get_async("/", handle_request)
        .get_async("/:secret", handle_request)
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

    handle_nip11(&ctx.env)
}

fn handle_options() -> Result<Response> {
    create_cors_response(Response::ok("")?)
}

fn handle_auth(_req: &Request, ctx: &RouteContext<()>) -> Result<Option<Response>> {
    let config = CloudflareConfig::from_env(&ctx.env);
    let policy = auth::from_config(&config);
    let provided_secret = ctx.param("secret").map(|s| s.as_str()).unwrap_or_default();

    match policy.check_access(provided_secret) {
        Ok(()) => Ok(None),
        Err(error) => {
            // The caller gets the same 404 either way; only the operator needs
            // to know the relay is closed because it was never configured.
            if error == AuthError::NotConfigured {
                log_error!(
                    "{error}: set AUTH_RELAY_SECRET, or AUTH_PUBLIC=true to run a public relay"
                );
            }
            Ok(Some(apply_security_headers(Response::error(
                "Not Found",
                404,
            )?)?))
        }
    }
}

fn is_websocket_upgrade(req: &Request) -> bool {
    req.headers()
        .get("Upgrade")
        .ok()
        .flatten()
        .is_some_and(|u| u.to_lowercase() == "websocket")
}

async fn handle_upgrade(req: Request, ctx: &RouteContext<()>) -> Result<Response> {
    crate::cloudflare::transport::connect(req, &ctx.env).await
}

/// NIP-11 relay information document.
///
/// The `description` states the supported scope, since the relay serves plain
/// NWC connections only: a wallet announces itself by publishing a `kind`
/// 13194 event, and cached events are replayed to `authors` filters. The
/// wallet-auth discovery pattern (a wallet announcing itself to an app through
/// a `p` tag) is not served; stating that here lets a client author check the
/// relay's scope without reading its source.
fn handle_nip11(env: &Env) -> Result<Response> {
    let config = CloudflareConfig::from_env(env);
    let info = serde_json::json!({
        "name": "mekhala",
        "description": "Nostr Wallet Connect relay. Supports plain NWC connection strings: cached kind 13194 wallet info events are replayed for `authors` filters. Wallet-auth discovery, where a wallet announces itself to an app through a `p` tag on its info event, is not supported.",
        "supported_nips": [1, 9, 11, 47],
        "limitation": {
            "max_subscriptions": config.max_subscriptions_per_connection,
            "max_subid_length": crate::nostr::limits::MAX_SUB_ID_LENGTH,
            "max_filters": config.max_filters_per_req,
            "max_event_tags": config.max_event_tags,
            "max_content_length": config.max_content_length,
        }
    });
    crate::cloudflare::transport::create_response(&info, "application/nostr+json")
}
