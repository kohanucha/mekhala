use std::collections::HashMap;
use std::cell::RefCell;
use serde::{Deserialize, Serialize};
use worker::*;
use lru::LruCache;
use std::num::NonZeroUsize;

mod relay;
mod utils;
mod nwc_client;
mod lnurl;
mod nwc_relay;

use relay::{ClientMessage, RelayMessage, Filter, Event, Limits};
use utils::{create_cors_response, constant_time_eq};
use nwc_relay::RelayHandler;

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
    utils::set_panic_hook();

    let router = Router::new();

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
        return create_cors_response(Response::ok("")?);
    }

    let expected_secret = ctx.var("RELAY_SECRET").map(|v| v.to_string()).unwrap_or_default();
    let provided_secret = ctx.param("secret").map(|s| s.as_str()).unwrap_or_default();

    if !expected_secret.is_empty() && !constant_time_eq(provided_secret, &expected_secret) {
        return Response::error("Unauthorized", 401);
    }

    if let Ok(Some(upgrade)) = req.headers().get("Upgrade") {
        if upgrade.to_lowercase() == "websocket" {
            let region = ctx.var("WALLET_REGION").map(|v| v.to_string()).ok();
            let stub = utils::get_durable_stub(&ctx.env, region)?;
            return stub.fetch_with_request(req).await;
        }
    }

    relay::handle_get_info()
}

#[durable_object]
pub struct NwcRelay {
    state: State,
    active_wallets: RefCell<HashMap<String, usize>>,
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

        let mut wallets = HashMap::new();
        for ws in state.get_websockets() {
            if let Ok(Some(conn_state)) = ws.deserialize_attachment::<ConnectionState>() {
                Self::rebuild_wallets_from_state(&mut wallets, &conn_state);
            }
        }

        Self { 
            state, 
            active_wallets: RefCell::new(wallets), 
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
            let is_online = self.active_wallets.borrow().get(pubkey).copied().unwrap_or(0) > 0;
            return Response::ok(if is_online { "OK" } else { "OFFLINE" });
        }

        if self.state.get_websockets().len() >= self.max_connections {
            return Response::error("Too Many Requests", 429);
        }

        let WebSocketPair { client, server } = WebSocketPair::new()?;
        server.serialize_attachment(&ConnectionState { limits: self.limits, ..Default::default() })?;
        self.state.accept_web_socket(&server);
        Response::from_websocket(client)
    }

    async fn websocket_message(&self, ws: WebSocket, message: WebSocketIncomingMessage) -> Result<()> {
        if let WebSocketIncomingMessage::String(text) = message {
            // Enforcement: Message size limit (64KB)
            if text.len() > 65536 {
                let _ = ws.send_with_str(&RelayMessage::Notice("error: message too large".into()).to_json());
                return Ok(());
            }

            let client_msg = ClientMessage::from_json(&text).map_err(|e| worker::Error::from(e.to_string()))?;
            let mut conn_state: ConnectionState = ws.deserialize_attachment()?.unwrap_or_default();
            
            let handler = RelayHandler {
                state: &self.state,
                active_wallets: &self.active_wallets,
                verification_cache: &self.verification_cache,
            };

            match client_msg {
                ClientMessage::Event(e) => handler.handle_event(&ws, &mut conn_state, e).await?,
                ClientMessage::Req(id, f) => handler.handle_req(&ws, &mut conn_state, id, f).await?,
                ClientMessage::Close(id) => {
                    if let Some(filters) = conn_state.subscriptions.remove(&id) {
                        ws.serialize_attachment(&conn_state)?;
                        handler.update_wallet_count(&filters, false);
                        handler.update_websocket_tags(&ws, &conn_state);
                    }
                }
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
    fn rebuild_wallets_from_state(wallets: &mut HashMap<String, usize>, conn_state: &ConnectionState) {
        for filters in conn_state.subscriptions.values() {
            for filter in filters {
                for pubkey in filter.pubkeys() {
                    *wallets.entry(pubkey).or_insert(0) += 1;
                }
            }
        }
    }

    async fn handle_websocket_close(&self, ws: &WebSocket) -> Result<()> {
        let conn_state: ConnectionState = ws.deserialize_attachment()?.unwrap_or_default();
        let filters: Vec<Filter> = conn_state.subscriptions.into_values().flatten().collect();
        
        let handler = RelayHandler {
            state: &self.state,
            active_wallets: &self.active_wallets,
            verification_cache: &self.verification_cache,
        };
        handler.update_wallet_count(&filters, false);
        Ok(())
    }
}

#[cfg(test)]
mod nwc_relay_tests {
    use super::*;
    use crate::relay::Filter;

    #[test]
    fn test_rebuild_wallets_from_state() {
        let mut wallets = HashMap::new();
        let mut conn_state = ConnectionState::default();
        let mut filters = Vec::new();
        filters.push(Filter {
            authors: Some(vec!["pub1".into()]),
            p_tags: Some(vec!["pub2".into()]),
            ..Default::default()
        });
        conn_state.subscriptions.insert("sub1".into(), filters);
        NwcRelay::rebuild_wallets_from_state(&mut wallets, &conn_state);
        assert_eq!(wallets.get("pub1"), Some(&1));
        assert_eq!(wallets.get("pub2"), Some(&1));
    }

    #[test]
    fn test_rebuild_wallets_multiple_subscriptions() {
        let mut wallets = HashMap::new();
        let mut conn_state = ConnectionState::default();
        conn_state.subscriptions.insert("sub1".into(), vec![Filter { p_tags: Some(vec!["pub1".into()]), ..Default::default() }]);
        conn_state.subscriptions.insert("sub2".into(), vec![Filter { p_tags: Some(vec!["pub1".into()]), ..Default::default() }]);
        NwcRelay::rebuild_wallets_from_state(&mut wallets, &conn_state);
        assert_eq!(wallets.get("pub1"), Some(&2));
    }
}
