use crate::auth::AuthError;
use crate::cloudflare::admin;
use crate::cloudflare::auth;
use crate::cloudflare::config::CloudflareConfig;
use crate::cloudflare::handler::LnAddressHandler;
use crate::cloudflare::{apply_security_headers, create_cors_response, CloudflareKvStore};
use worker::{Env, Error, Method, Request, Response, Result, RouteContext, Router};

pub async fn run(req: Request, env: Env) -> Result<Response> {
    let router = Router::new();

    router
        // The admin routes are registered first so the literal paths always
        // win over the `/:secret` relay-password route.
        .get_async("/admin", |_req, ctx| async move {
            admin::handle_page(&ctx.env).await
        })
        .get_async("/admin/", |_req, ctx| async move {
            admin::handle_page(&ctx.env).await
        })
        .get_async("/admin/api/registrations", |req, ctx| async move {
            admin::handle_list(&req, &ctx.env).await
        })
        .post_async("/admin/api/registrations", |req, ctx| async move {
            admin::handle_create(req, &ctx.env).await
        })
        .delete_async(
            "/admin/api/registrations/:username",
            |req, ctx| async move {
                let username = ctx
                    .param("username")
                    .ok_or_else(|| Error::from("Missing username"))?
                    .to_owned();
                admin::handle_delete(&req, &ctx.env, &username).await
            },
        )
        .post_async("/admin/api/reseal", |req, ctx| async move {
            admin::handle_reseal(&req, &ctx.env).await
        })
        .get_async("/", handle_request)
        .get_async("/:secret", handle_request)
        .get_async("/.well-known/lnurlp/:username", |req, ctx| async move {
            let kv = ctx.env.kv("MEKHALA_NWC_KV")?;
            let config = CloudflareConfig::from_env(&ctx.env);
            let kv_store = CloudflareKvStore::new(kv, config.ln_address_keys);
            let handler = LnAddressHandler::new(&kv_store, config.lnaddress_policy());
            let username = ctx
                .param("username")
                .ok_or_else(|| Error::from("Missing username"))?;
            if !crate::lnaddress::is_valid_username(username) {
                return create_cors_response(
                    Response::from_json(
                        &serde_json::json!({ "status": "ERROR", "reason": "Not Found" }),
                    )?
                    .with_status(404),
                );
            }
            match handler.handle_pay_request(req, username).await {
                Ok(resp) => Ok(resp),
                Err(e) => {
                    let error_body =
                        serde_json::json!({ "status": "ERROR", "reason": e.to_string() });
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
        Err(AuthError::Forbidden) => Ok(Some(apply_security_headers(Response::error(
            "Not Found",
            404,
        )?)?)),
        Ok(_) => Ok(None),
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
            "max_event_tags": config.max_event_tags,
            "max_content_length": config.max_content_length,
        }
    });
    crate::cloudflare::transport::create_response(&info, "application/nostr+json")
}
