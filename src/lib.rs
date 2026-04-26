use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use worker::*;

mod relay;
mod utils;

use relay::{ClientMessage, RelayMessage, Filter, Event, KIND_NWC_INFO};

/// State associated with a single WebSocket connection
#[derive(Serialize, Deserialize, Default)]
pub struct ConnectionState {
    pub subscriptions: HashMap<String, Vec<Filter>>,
    pub info_event: Option<Event>,
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    utils::set_panic_hook();

    let router = Router::new();

    router
        .get_async("/", handle_request)
        .get_async("/:secret", handle_request)
        .run(req, env)
        .await
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
            let namespace = ctx.env.durable_object("NWC_RELAY")?;
            let region = ctx.var("WALLET_REGION").map(|v| v.to_string()).unwrap_or_default();

            let stub = if !region.is_empty() {
                namespace.get_by_name_with_location_hint("GLOBAL", &region)?
            } else {
                namespace.id_from_name("GLOBAL")?.get_stub()?
            };
            return stub.fetch_with_request(req).await;
        }
    }

    // 4. Fallback to NIP-11 Metadata
    handle_get_info()
}

fn handle_get_info() -> Result<Response> {
    let info = serde_json::json!({
        "supported_nips": [1, 11, 47]
    });
    create_cors_response(Response::from_json(&info)?)
}

fn create_cors_response(response: Response) -> Result<Response> {
    let headers = response.headers();
    let mut new_headers = headers.clone();
    new_headers.set("Access-Control-Allow-Origin", "*")?;
    new_headers.set("Access-Control-Allow-Methods", "GET, OPTIONS")?;
    new_headers.set("Access-Control-Allow-Headers", "*")?;
    new_headers.set("Content-Type", "application/nostr+json")?;
    Ok(response.with_headers(new_headers))
}

/// Constant-time string comparison to prevent timing attacks
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() { return false; }
    a.bytes().zip(b.bytes()).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[durable_object]
pub struct NwcRelay {
    state: State,
    _env: Env,
}

impl DurableObject for NwcRelay {
    fn new(state: State, env: Env) -> Self {
        Self { state, _env: env }
    }

    async fn fetch(&self, _req: Request) -> Result<Response> {
        let WebSocketPair { client, server } = WebSocketPair::new()?;
        
        let initial_state = ConnectionState::default();
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
}

impl NwcRelay {
    async fn handle_event(&self, ws: &WebSocket, conn_state: &mut ConnectionState, event: Event) -> Result<()> {
        let current_time = (Date::now().as_millis() / 1000) as u64;
        
        // 1. Verify Event
        if let Err(reason) = event.verify(current_time) {
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
        if filters.iter().any(|f| !f.is_valid()) {
            let msg = "restricted: NIP-47 subscriptions must be narrowed by author, p-tag, or e-tag";
            let _ = ws.send_with_str(&RelayMessage::Closed(sub_id, msg.into()).to_json());
            return Ok(());
        }

        // 3. Save subscription
        conn_state.subscriptions.insert(sub_id.clone(), filters.clone());
        ws.serialize_attachment(conn_state)?;

        // 4. Serve cached Info Events
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

        // 5. Send EOSE
        let _ = ws.send_with_str(&RelayMessage::Eose(sub_id).to_json());
        Ok(())
    }

    async fn handle_close(&self, ws: &WebSocket, conn_state: &mut ConnectionState, sub_id: String) -> Result<()> {
        if conn_state.subscriptions.remove(&sub_id).is_some() {
            let _ = ws.serialize_attachment(conn_state);
        }
        Ok(())
    }
}
