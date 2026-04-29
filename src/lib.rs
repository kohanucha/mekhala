use std::collections::HashMap;
use std::cell::RefCell;
use serde::{Deserialize, Serialize};
use worker::*;

mod relay;
mod utils;
mod nwc_client;
mod lnurl;

use relay::{ClientMessage, RelayMessage, Filter, Event, KIND_NWC_INFO, Limits};
use utils::{create_cors_response, constant_time_eq};

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

        Self { state, active_wallets: RefCell::new(HashMap::new()), limits, max_connections }
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
    fn update_wallet_count(&self, filters: &[Filter], increment: bool) {
        let mut wallets = self.active_wallets.borrow_mut();
        for filter in filters {
            if let Some(p_tags) = &filter.p_tags {
                for pubkey in p_tags {
                    if increment {
                        *wallets.entry(pubkey.clone()).or_insert(0) += 1;
                    } else if let Some(count) = wallets.get_mut(pubkey) {
                        *count = count.saturating_sub(1);
                    }
                }
            }
        }
        if !increment {
            wallets.retain(|_, v| *v > 0);
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
        
        // 1. Verify Event
        if let Err(reason) = event.verify(current_time, &conn_state.limits) {
            let _ = ws.send_with_str(&RelayMessage::Ok(event.id, false, reason.to_string()).to_json());
            return Ok(());
        }

        // 2. Cache Info Event (if applicable)
        if event.kind == KIND_NWC_INFO {
            conn_state.info_event = Some(event.clone());
            let _ = ws.serialize_attachment(conn_state);
        }

        // 3. Acknowledge Event
        let _ = ws.send_with_str(&RelayMessage::Ok(event.id.clone(), true, "".into()).to_json());

        // 4. Broadcast to matching subscribers
        for other_ws in self.state.get_websockets() {
            let other_state: ConnectionState = match other_ws.deserialize_attachment() {
                Ok(Some(s)) => s,
                _ => continue,
            };
            for (sub_id, filters) in &other_state.subscriptions {
                if filters.iter().any(|f| f.matches(&event)) {
                    let _ = other_ws.send_with_str(&RelayMessage::Event(sub_id.clone(), event.clone()).to_json());
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
        ws.serialize_attachment(conn_state)?;

        // 4. Track active wallet connections
        self.update_wallet_count(&filters, true);

        // 5. Serve cached Info Events
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
            let _ = ws.serialize_attachment(conn_state);
            self.update_wallet_count(&filters, false);
        }
        Ok(())
    }
}
