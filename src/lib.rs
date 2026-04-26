use std::collections::HashMap;
use worker::*;

mod relay;
mod utils;

use relay::{ClientMessage, RelayMessage, Filter};

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
    let expected_secret = ctx.var("RELAY_SECRET").map(|v| v.to_string()).unwrap_or_default();
    let provided_secret = ctx.param("secret").map(|s| s.as_str()).unwrap_or_default();

    if !expected_secret.is_empty() && provided_secret != expected_secret {
        return Response::error("Unauthorized", 401);
    }

    if let Ok(Some(upgrade)) = req.headers().get("Upgrade") {
        if upgrade.to_lowercase() == "websocket" {
            let namespace = ctx.env.durable_object("NWC_EDGE_RELAY")?;
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

fn handle_get_info(req: Request, _ctx: RouteContext<()>) -> Result<Response> {
    if req.headers().get("Accept")?.as_deref() == Some("application/nostr+json") {
        let info = serde_json::json!({
            "supported_nips": [1, 11, 47]
        });

        let headers = Headers::new();
        headers.set("Content-Type", "application/nostr+json")?;
        headers.set("Access-Control-Allow-Origin", "*")?;

        return Ok(Response::from_json(&info)?.with_headers(headers));
    }

    Response::error("Please use a Nostr client to connect.", 400)
}

#[durable_object]
pub struct NwcEdgeRelay {
    state: State,
    _env: Env,
}

impl DurableObject for NwcEdgeRelay {
    fn new(state: State, env: Env) -> Self {
        Self {
            state,
            _env: env,
        }
    }

    async fn fetch(&self, _req: Request) -> Result<Response> {
        let WebSocketPair { client, server } = WebSocketPair::new()?;
        
        // Use Hibernation API: accept the websocket and manage it via the runtime.
        // We store the client's subscriptions (Filter map) in the WebSocket's "attachment".
        // This allows the Durable Object to be evicted from memory and still recover its state.
        let initial_subscriptions: HashMap<String, Vec<Filter>> = HashMap::new();
        server.serialize_attachment(&initial_subscriptions)?;
        
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
                None => return Ok(()),
            };

            // Get current subscriptions from the WebSocket attachment
            let mut subscriptions: HashMap<String, Vec<Filter>> = ws.deserialize_attachment()?.unwrap_or_default();

            match client_msg {
                ClientMessage::Event(event) => {
                    if !event.verify() {
                        let _ = ws.send_with_str(&RelayMessage::Ok(event.id, false, "invalid: signature verification failed".into()).to_json());
                        return Ok(());
                    }

                    let _ = ws.send_with_str(&RelayMessage::Ok(event.id.clone(), true, "".into()).to_json());

                    // Broadcast to ALL connected websockets managed by this Durable Object
                    for other_ws in self.state.get_websockets() {
                        let other_subs: HashMap<String, Vec<Filter>> = other_ws.deserialize_attachment()?.unwrap_or_default();
                        for (sub_id, filters) in other_subs.iter() {
                            if filters.iter().any(|f| f.matches(&event)) {
                                let _ = other_ws.send_with_str(&RelayMessage::Event(sub_id.clone(), event.clone()).to_json());
                            }
                        }
                    }
                }
                ClientMessage::Req(sub_id, filters) => {
                    if filters.iter().any(|f| !f.is_valid()) {
                        let _ = ws.send_with_str(&RelayMessage::Closed(sub_id, "restricted: NIP-47 subscriptions must be narrowed by author or p-tag".into()).to_json());
                        return Ok(());
                    }
                    subscriptions.insert(sub_id.clone(), filters);
                    if let Err(e) = ws.serialize_attachment(&subscriptions) {
                        let _ = ws.send_with_str(&RelayMessage::Notice(format!("error: failed to save subscription: {}", e)).to_json());
                        return Ok(());
                    }
                    let _ = ws.send_with_str(&RelayMessage::Eose(sub_id).to_json());
                }
                ClientMessage::Close(sub_id) => {
                    if subscriptions.remove(&sub_id).is_some() {
                        let _ = ws.serialize_attachment(&subscriptions);
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
