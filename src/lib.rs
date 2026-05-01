use std::collections::HashMap;
use std::cell::RefCell;
use serde::{Deserialize, Serialize};
use worker::*;
use lru::LruCache;
use std::num::NonZeroUsize;

mod domain;
mod relay;
mod platform;
mod enforcement;
mod nwc_client;
mod lnurl;
mod pipeline;
mod router;
mod protocol;
mod auth;
mod connection;

use domain::{Filter, Event, Limits};
use relay::{ClientMessage, RelayMessage};
use platform::Platform;
use enforcement::Enforcement;
use pipeline::EventPipeline;
use router::Router;
use auth::Authenticator;
use connection::Connection;

/// State associated with a single WebSocket connection
#[derive(Serialize, Deserialize, Clone)]
pub struct ConnectionState {
    pub subscriptions: HashMap<String, Vec<Filter>>,
    pub info_event: Option<Event>,
    pub limits: Limits,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self {
            subscriptions: HashMap::new(),
            info_event: None,
            limits: Limits::default(),
        }
    }
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Platform::set_panic_hook();

    let router = worker::Router::new();

    router
        .get_async("/", handle_request)
        .get_async("/.well-known/lnurlp/:username", lnurl::handle_lnurlp)
        .get_async("/lnurlp/:username/callback", lnurl::handle_lnurlp_callback)
        .get_async("/:secret", handle_request)
        .run(req, env)
        .await
}

async fn handle_request(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if req.method() == Method::Options {
        return Platform::create_cors_response(Response::ok("")?);
    }

    let auth = Authenticator::from_env(&ctx.env);
    let provided_secret = ctx.param("secret").map(|s| s.as_str()).unwrap_or_default();

    if !auth.is_authorized(provided_secret) {
        return Platform::apply_security_headers(Response::error("Unauthorized", 401)?);
    }

    if let Ok(Some(upgrade)) = req.headers().get("Upgrade") {
        if upgrade.to_lowercase() == "websocket" {
            let region = ctx.var("WALLET_REGION").map(|v| v.to_string()).ok();
            let stub = Platform::get_durable_stub(&ctx.env, region)?;
            return stub.fetch_with_request(req).await;
        }
    }

    relay::handle_get_info()
}

#[durable_object]
pub struct NwcRelay {
    state: State,
    router: Router,
    verification_cache: RefCell<LruCache<String, Result<(), String>>>,
    limits: Limits,
    max_connections: usize,
}

impl DurableObject for NwcRelay {
    fn new(state: State, env: Env) -> Self {
        let get_var = |name, default| env.var(name).map(|v| v.to_string().parse().unwrap_or(default)).unwrap_or(default);
        
        let limits = Limits {
            max_filter_items: get_var("MAX_FILTER_ITEMS", 100),
            max_event_tags: get_var("MAX_EVENT_TAGS", 100),
            max_content_length: get_var("MAX_CONTENT_LENGTH", 32768),
        };

        Self { 
            router: Router::new(&state),
            state, 
            verification_cache: RefCell::new(LruCache::new(NonZeroUsize::new(500).unwrap())),
            limits, 
            max_connections: get_var("MAX_CONNECTIONS", 100),
        }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path();

        if path.starts_with("/check/") {
            let pubkey = path.strip_prefix("/check/").unwrap_or("");
            let is_online = self.router.is_wallet_online(pubkey);
            return Platform::apply_security_headers(Response::ok(if is_online { "OK" } else { "OFFLINE" })?);
        }

        // Delegate connection acceptance to the Connection module
        Connection::accept(&self.state, self.limits, self.max_connections)
    }

    async fn websocket_message(&self, ws: WebSocket, message: WebSocketIncomingMessage) -> Result<()> {
        let client_msg = match Enforcement::parse_incoming(&message) {
            Ok(msg) => msg,
            Err(e) => {
                let _ = ws.send_with_str(&RelayMessage::Notice(e).to_json());
                return Ok(());
            }
        };

        // Use the Router's high-leverage Speed Layer
        let mut conn_state = self.router.get_state(&ws)?;
        
        let pipeline = EventPipeline::new(
            &self.state,
            &self.router,
            &self.verification_cache,
        );

        match client_msg {
            ClientMessage::Event(e) => pipeline.handle_event(&ws, &mut conn_state, e).await?,
            ClientMessage::Req(id, f) => pipeline.handle_req(&ws, &mut conn_state, id, f).await?,
            ClientMessage::Close(id) => {
                self.router.unsubscribe(&self.state, &ws, Some(id), &mut conn_state)?;
            }
        }

        Ok(())
    }

    async fn websocket_close(&self, ws: WebSocket, _: usize, _: String, _: bool) -> Result<()> {
        self.handle_websocket_close(&ws).await
    }

    async fn websocket_error(&self, ws: WebSocket, _: Error) -> Result<()> {
        self.handle_websocket_close(&ws).await
    }
}

impl NwcRelay {
    async fn handle_websocket_close(&self, ws: &WebSocket) -> Result<()> {
        let mut conn_state = self.router.get_state(ws)?;
        self.router.unsubscribe(&self.state, ws, None, &mut conn_state)
    }
}
