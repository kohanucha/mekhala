use std::collections::HashMap;
use std::cell::RefCell;
use serde::{Deserialize, Serialize};
use worker::*;
use lru::LruCache;
use std::num::NonZeroUsize;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen::prelude::*;

mod relay;
mod utils;
mod nwc_client;
mod lnurl;

use relay::{ClientMessage, RelayMessage, Filter, Event, KIND_NWC_INFO, Limits};
use utils::{create_cors_response, constant_time_eq, DurableObjectStateExt, tags_supported};

/// State associated with a single WebSocket connection
#[derive(Serialize, Deserialize)]
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

    let res = router
        .get_async("/", handle_request)
        .get_async("/.well-known/lnurlp/:username", lnurl::handle_lnurlp)
        .get_async("/lnurlp/:username/callback", lnurl::handle_lnurlp_callback)
        .get_async("/:secret", handle_request)
        .run(req, env)
        .await;

    match res {
        Ok(r) => Ok(r),
        Err(e) => {
            let error_body = serde_json::json!({ "error": e.to_string() });
            create_cors_response(Response::from_json(&error_body)?.with_status(500))
        }
    }
}

async fn handle_request(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    // 1. Handle CORS Preflight
    if req.method() == Method::Options {
        return create_cors_response(Response::ok("")?);
    }

    // 2. Authorization Check
    let expected_secret = ctx.var("RELAY_SECRET").map(|v| v.to_string()).unwrap_or_default();
    let provided_secret = ctx.param("secret").map(|s| s.as_str()).unwrap_or_default();

    if !expected_secret.is_empty() && !constant_time_eq(provided_secret, &expected_secret) {
        return Response::error("Unauthorized", 401);
    }

    // 3. Handle WebSocket Upgrade
    if let Ok(Some(upgrade)) = req.headers().get("Upgrade") {
        if upgrade.to_lowercase() == "websocket" {
            let region = ctx.var("WALLET_REGION").map(|v| v.to_string()).ok();
            let stub = utils::get_durable_stub(&ctx.env, region)?;
            return stub.fetch_with_request(req).await;
        }
    }

    // 4. Fallback to NIP-11 Metadata
    relay::handle_get_info()
}

#[durable_object]
pub struct NwcRelay {
    state: State,
    active_wallets: std::cell::RefCell<HashMap<String, usize>>,
    verification_cache: RefCell<LruCache<String, Result<(), String>>>,
    limits: Limits,
    max_connections: usize,
}

impl DurableObject for NwcRelay {
    fn new(state: State, env: Env) -> Self {
        let max_connections = env.var("MAX_CONNECTIONS").map(|v| v.to_string().parse().unwrap_or(100)).unwrap_or(100);
        let max_filter_items = env.var("MAX_FILTER_ITEMS").map(|v| v.to_string().parse().unwrap_or(100)).unwrap_or(100);
        let max_event_tags = env.var("MAX_EVENT_TAGS").map(|v| v.to_string().parse().unwrap_or(100)).unwrap_or(100);
        let max_content_length = env.var("MAX_CONTENT_LENGTH").map(|v| v.to_string().parse().unwrap_or(32768)).unwrap_or(32768);

        let limits = Limits {
            max_filter_items,
            max_event_tags,
            max_content_length,
        };

        // Rebuild active wallet map from hibernated WebSockets
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
            max_connections 
        }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path();

        if path.starts_with("/check/") {
            let pubkey = path.strip_prefix("/check/").unwrap_or("");
            if self.active_wallets.borrow().get(pubkey).copied().unwrap_or(0) > 0 {
                return Response::ok("OK");
            } else {
                return Response::ok("OFFLINE");
            }
        }

        // Limit concurrent connections (personal use optimization)
        if self.state.get_websockets().len() >= self.max_connections {
            return Response::error("Too Many Requests", 429);
        }

        let WebSocketPair { client, server } = WebSocketPair::new()?;
        
        let initial_state = ConnectionState {
            limits: self.limits,
            ..Default::default()
        };
        server.serialize_attachment(&initial_state)?;
        
        self.state.accept_web_socket(&server);
        Response::from_websocket(client)
    }

    async fn websocket_message(&self, ws: WebSocket, message: WebSocketIncomingMessage) -> Result<()> {
        if let WebSocketIncomingMessage::String(text) = message {
            // Enforcement: Message size limit
            if text.len() > 65536 {
                let _ = ws.send_with_str(&RelayMessage::Notice("error: message too large".into()).to_json());
                return Ok(());
            }

            // Parse message
            let client_msg = match ClientMessage::from_json(&text) {
                Ok(m) => m,
                Err(e) => {
                    let _ = ws.send_with_str(&RelayMessage::Notice(e.to_string()).to_json());
                    return Ok(());
                }
            };

            // Get current connection state
            let mut conn_state: ConnectionState = ws.deserialize_attachment()?.unwrap_or_default();

            // Delegate message handling
            match client_msg {
                ClientMessage::Event(e) => self.handle_event(&ws, &mut conn_state, e).await?,
                ClientMessage::Req(sub_id, filters) => self.handle_req(&ws, &mut conn_state, sub_id, filters).await?,
                ClientMessage::Close(sub_id) => self.handle_close(&ws, &mut conn_state, sub_id).await?,
            }
        }
        Ok(())
    }

    async fn websocket_close(&self, ws: WebSocket, _code: usize, _reason: String, _was_clean: bool) -> Result<()> {
        self.handle_websocket_close(&ws).await
    }

    async fn websocket_error(&self, ws: WebSocket, _error: Error) -> Result<()> {
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

    fn update_wallet_count(&self, filters: &[Filter], increment: bool) {
        let mut wallets = self.active_wallets.borrow_mut();
        for filter in filters {
            let mut keys = Vec::new();
            if let Some(p_tags) = &filter.p_tags {
                keys.extend(p_tags.clone());
            }
            if let Some(authors) = &filter.authors {
                keys.extend(authors.clone());
            }

            for pubkey in keys {
                if increment {
                    *wallets.entry(pubkey).or_insert(0) += 1;
                } else if let Some(count) = wallets.get_mut(&pubkey) {
                    *count = count.saturating_sub(1);
                }
            }
        }
        if !increment {
            wallets.retain(|_, v| *v > 0);
        }
    }

    fn update_websocket_tags(&self, ws: &WebSocket, conn_state: &ConnectionState) {
        let mut unique_pubkeys = std::collections::HashSet::new();
        for filters in conn_state.subscriptions.values() {
            for filter in filters {
                for pubkey in filter.pubkeys() {
                    unique_pubkeys.insert(pubkey);
                }
            }
        }

        let tags = js_sys::Array::new();
        for pubkey in unique_pubkeys.into_iter().take(10) {
            tags.push(&JsValue::from_str(&pubkey));
        }

        // Access the raw JS state object (DurableObjectState is a JsValue)
        let state_js: &JsValue = unsafe { std::mem::transmute(&self.state) };
        if tags_supported(state_js) {
            let state_ext: &DurableObjectStateExt = state_js.unchecked_ref();
            state_ext.set_websocket_tags(ws.as_ref(), tags);
        }
    }

    async fn handle_websocket_close(&self, ws: &WebSocket) -> Result<()> {
        let conn_state: ConnectionState = ws.deserialize_attachment()?.unwrap_or_default();
        let filters: Vec<Filter> = conn_state.subscriptions.into_values().flatten().collect();
        self.update_wallet_count(&filters, false);
        Ok(())
    }

    async fn handle_event(&self, ws: &WebSocket, conn_state: &mut ConnectionState, event: Event) -> Result<()> {
        let current_time = (Date::now().as_millis() / 1000) as u64;
        
        // 1. Verify Event (with cache)
        let cached_res = self.verification_cache.borrow_mut().get(&event.id).cloned();
        let verification_result = if let Some(res) = cached_res {
            res
        } else {
            let res = event.verify(current_time, &conn_state.limits).map_err(|e| e.to_string());
            self.verification_cache.borrow_mut().put(event.id.clone(), res.clone());
            res
        };

        if let Err(reason) = verification_result {
            let _ = ws.send_with_str(&RelayMessage::Ok(event.id, false, reason).to_json());
            return Ok(());
        }

        // 2. Cache Info Event (if applicable)
        if event.kind == KIND_NWC_INFO {
            conn_state.info_event = Some(event.clone());
            let _ = ws.serialize_attachment(&*conn_state);
        }

        // 3. Acknowledge Event
        let _ = ws.send_with_str(&RelayMessage::Ok(event.id.clone(), true, "".into()).to_json());

        // 4. Broadcast to matching subscribers using WebSocket Tags
        let state_js: &JsValue = unsafe { std::mem::transmute(&self.state) };
        let mut target_websockets: Vec<WebSocket> = Vec::new();

        let mut add_if_unique = |ws_js: JsValue| {
            if let Ok(web_sys_ws) = ws_js.dyn_into::<worker::web_sys::WebSocket>() {
                if !target_websockets.iter().any(|w| {
                    let w_js: &JsValue = w.as_ref();
                    let target_js: &JsValue = web_sys_ws.as_ref();
                    w_js == target_js
                }) {
                    target_websockets.push(WebSocket::from(web_sys_ws));
                }
            }
        };

        if tags_supported(state_js) {
            let state_ext: &DurableObjectStateExt = state_js.unchecked_ref();
            let websockets_array = |tag: Option<&str>| state_ext.get_websockets(tag);

            // Check author tag
            for ws_js in websockets_array(Some(&event.pubkey)).iter() {
                add_if_unique(ws_js);
            }

            // Check p-tags
            for tag in &event.tags {
                if tag.len() >= 2 && tag[0].as_str() == Some("p") {
                    if let Some(p_pubkey) = tag[1].as_str() {
                        for ws_js in websockets_array(Some(p_pubkey)).iter() {
                            add_if_unique(ws_js);
                        }
                    }
                }
            }
        } else {
            // Fallback: Get ALL WebSockets if tagging is not supported
            for ws in self.state.get_websockets() {
                target_websockets.push(ws);
            }
        }

        // Broadcast to identified targets
        for target_ws in &target_websockets {
            let other_state: ConnectionState = match target_ws.deserialize_attachment() {
                Ok(Some(s)) => s,
                _ => continue,
            };
            for (sub_id, filters) in &other_state.subscriptions {
                if filters.iter().any(|f| f.matches(&event)) {
                    let _ = target_ws.send_with_str(&RelayMessage::Event(sub_id.clone(), event.clone()).to_json());
                }
            }
        }

        Ok(())
    }

    async fn handle_req(&self, ws: &WebSocket, conn_state: &mut ConnectionState, sub_id: String, filters: Vec<Filter>) -> Result<()> {
        // 1. Anti-spam: Limit subscriptions
        if conn_state.subscriptions.len() >= 20 && !conn_state.subscriptions.contains_key(&sub_id) {
            let _ = ws.send_with_str(&RelayMessage::Closed(sub_id, "rate-limited: max 20 subscriptions".into()).to_json());
            return Ok(());
        }

        // 2. Protocol: Validate NIP-47 filter strictness
        if filters.iter().any(|f| !f.is_valid(&conn_state.limits)) {
            let msg = "restricted: NIP-47 subscriptions must be narrowed by author, p-tag, or e-tag";
            let _ = ws.send_with_str(&RelayMessage::Closed(sub_id, msg.into()).to_json());
            return Ok(());
        }

        // 3. Save subscription
        conn_state.subscriptions.insert(sub_id.clone(), filters.clone());
        ws.serialize_attachment(&*conn_state)?;

        // 4. Track active wallet connections
        self.update_wallet_count(&filters, true);

        // 5. Update WebSocket Tags for O(1) routing
        self.update_websocket_tags(ws, conn_state);

        // 6. Serve cached Info Events
        if filters.iter().any(|f| f.kinds.as_ref().map_or(false, |k| k.contains(&KIND_NWC_INFO))) {
            for other_ws in self.state.get_websockets() {
                let other_state: ConnectionState = match other_ws.deserialize_attachment() {
                    Ok(Some(s)) => s,
                    _ => continue,
                };
                if let Some(cached_info) = &other_state.info_event {
                    if filters.iter().any(|f| f.matches(cached_info)) {
                        let _ = ws.send_with_str(&RelayMessage::Event(sub_id.clone(), cached_info.clone()).to_json());
                    }
                }
            }
        }

        // 6. Send EOSE
        let _ = ws.send_with_str(&RelayMessage::Eose(sub_id).to_json());
        Ok(())
    }

    async fn handle_close(&self, ws: &WebSocket, conn_state: &mut ConnectionState, sub_id: String) -> Result<()> {
        if let Some(filters) = conn_state.subscriptions.remove(&sub_id) {
            let _ = ws.serialize_attachment(&*conn_state);
            self.update_wallet_count(&filters, false);
            self.update_websocket_tags(ws, conn_state);
        }
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
        assert_eq!(wallets.len(), 2);
    }

    #[test]
    fn test_rebuild_wallets_multiple_subscriptions() {
        let mut wallets = HashMap::new();
        let mut conn_state = ConnectionState::default();
        
        conn_state.subscriptions.insert("sub1".into(), vec![Filter {
            p_tags: Some(vec!["pub1".into()]),
            ..Default::default()
        }]);
        conn_state.subscriptions.insert("sub2".into(), vec![Filter {
            p_tags: Some(vec!["pub1".into()]),
            ..Default::default()
        }]);
        
        NwcRelay::rebuild_wallets_from_state(&mut wallets, &conn_state);
        
        assert_eq!(wallets.get("pub1"), Some(&2));
    }
}
