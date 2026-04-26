use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use worker::*;

mod relay;
mod utils;

use relay::{ClientMessage, RelayMessage, Filter, Event, KIND_NWC_INFO};

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
    if req.method() == Method::Options {
        let headers = Headers::new();
        headers.set("Access-Control-Allow-Origin", "*")?;
        headers.set("Access-Control-Allow-Methods", "GET, OPTIONS")?;
        headers.set("Access-Control-Allow-Headers", "*")?;
        return Ok(Response::ok("")?.with_headers(headers));
    }

    let expected_secret = ctx.var("RELAY_SECRET").map(|v| v.to_string()).unwrap_or_default();
    let provided_secret = ctx.param("secret").map(|s| s.as_str()).unwrap_or_default();

    if !expected_secret.is_empty() && !constant_time_eq(provided_secret, &expected_secret) {
        return Response::error("Unauthorized", 401);
    }

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
    handle_get_info(req, ctx)
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0;
    for (byte_a, byte_b) in a.bytes().zip(b.bytes()) {
        result |= byte_a ^ byte_b;
    }
    result == 0
}

fn handle_get_info(_req: Request, _ctx: RouteContext<()>) -> Result<Response> {
    let info = serde_json::json!({
        "supported_nips": [1, 11, 47]
    });

    let headers = Headers::new();
    headers.set("Content-Type", "application/nostr+json")?;
    headers.set("Access-Control-Allow-Origin", "*")?;

    Ok(Response::from_json(&info)?.with_headers(headers))
}

#[durable_object]
pub struct NwcRelay {
    state: State,
    _env: Env,
}

impl DurableObject for NwcRelay {
    fn new(state: State, env: Env) -> Self {
        Self {
            state,
            _env: env,
        }
    }

    async fn fetch(&self, _req: Request) -> Result<Response> {
        let WebSocketPair { client, server } = WebSocketPair::new()?;
        
        // Use Hibernation API: accept the websocket and manage it via the runtime.
        // We store the client's state (subscriptions and info event) in the WebSocket's "attachment".
        // This allows the Durable Object to survive hibernation cycles.
        let initial_state = ConnectionState::default();
        server.serialize_attachment(&initial_state)?;
        
        self.state.accept_web_socket(&server);

        Response::from_websocket(client)
    }

    async fn websocket_message(&self, ws: WebSocket, message: WebSocketIncomingMessage) -> Result<()> {
        if let WebSocketIncomingMessage::String(text) = message {
            if text.len() > 65536 {
                let _ = ws.send_with_str(&RelayMessage::Notice("error: message too large".into()).to_json());
                return Ok(());
            }

            let client_msg = match ClientMessage::from_json(&text) {
                Some(m) => m,
                None => {
                    let _ = ws.send_with_str(&RelayMessage::Notice("error: unparseable message or invalid JSON format".into()).to_json());
                    return Ok(());
                }
            };

            // Get current state from the WebSocket attachment
            let mut conn_state: ConnectionState = ws.deserialize_attachment()?.unwrap_or_default();

            match client_msg {
                ClientMessage::Event(event) => {
                    let current_time = (Date::now().as_millis() / 1000) as u64;
                    if let Err(reason) = event.verify(current_time) {
                        let _ = ws.send_with_str(&RelayMessage::Ok(event.id, false, reason).to_json());
                        return Ok(());
                    }

                    // Cache NIP-47 Info Event in the WebSocket attachment
                    if event.kind == KIND_NWC_INFO {
                        conn_state.info_event = Some(event.clone());
                        let _ = ws.serialize_attachment(&conn_state);
                    }

                    let _ = ws.send_with_str(&RelayMessage::Ok(event.id.clone(), true, "".into()).to_json());

                    // Broadcast to ALL connected websockets managed by this Durable Object
                    for other_ws in self.state.get_websockets() {
                        let other_state: ConnectionState = match other_ws.deserialize_attachment() {
                            Ok(Some(s)) => s,
                            _ => continue,
                        };
                        for (sub_id, filters) in other_state.subscriptions.iter() {
                            if filters.iter().any(|f| f.matches(&event)) {
                                let _ = other_ws.send_with_str(&RelayMessage::Event(sub_id.clone(), event.clone()).to_json());
                            }
                        }
                    }
                }
                ClientMessage::Req(sub_id, filters) => {
                    if conn_state.subscriptions.len() >= 20 && !conn_state.subscriptions.contains_key(&sub_id) {
                        let _ = ws.send_with_str(&RelayMessage::Closed(sub_id, "rate-limited: max 20 subscriptions".into()).to_json());
                        return Ok(());
                    }

                    if filters.iter().any(|f| !f.is_valid()) {
                        let _ = ws.send_with_str(&RelayMessage::Closed(sub_id, "restricted: NIP-47 subscriptions must be narrowed by author, p-tag, or e-tag".into()).to_json());
                        return Ok(());
                    }
                    conn_state.subscriptions.insert(sub_id.clone(), filters.clone());
                    if let Err(e) = ws.serialize_attachment(&conn_state) {
                        let _ = ws.send_with_str(&RelayMessage::Notice(format!("error: failed to save subscription: {}", e)).to_json());
                        return Ok(());
                    }

                    // Serve cached NIP-47 Info Events from all active connections
                    if filters.iter().any(|f| f.kinds.as_ref().map_or(false, |k| k.contains(&KIND_NWC_INFO))) {
                        for other_ws in self.state.get_websockets() {
                            let other_state: ConnectionState = match other_ws.deserialize_attachment() {
                                Ok(Some(s)) => s,
                                _ => continue,
                            };
                            if let Some(cached_info) = other_state.info_event {
                                if filters.iter().any(|f| f.matches(&cached_info)) {
                                    let _ = ws.send_with_str(&RelayMessage::Event(sub_id.clone(), cached_info).to_json());
                                }
                            }
                        }
                    }

                    let _ = ws.send_with_str(&RelayMessage::Eose(sub_id).to_json());
                }
                ClientMessage::Close(sub_id) => {
                    if conn_state.subscriptions.remove(&sub_id).is_some() {
                        let _ = ws.serialize_attachment(&conn_state);
                    }
                }
            }
        }
        Ok(())
    }

    async fn websocket_close(&self, _ws: WebSocket, _code: usize, _reason: String, _was_clean: bool) -> Result<()> {
        Ok(())
    }
}
