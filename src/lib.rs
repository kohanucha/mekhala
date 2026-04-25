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
        .get_async("/", |req, ctx| async move {
            if let Ok(Some(upgrade)) = req.headers().get("Upgrade") {
                if upgrade.to_lowercase() == "websocket" {
                    let namespace = ctx.env.durable_object("NWC_RELAY")?;
                    let stub = namespace.id_from_name("GLOBAL")?.get_stub()?;
                    return stub.fetch_with_request(req).await;
                }
            }
            handle_get_info(req, ctx)
        })
        .run(req, env)
        .await
}

fn handle_get_info(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    if req.headers().get("Accept")?.as_deref() == Some("application/nostr+json") {
        let name = ctx.env.var("RELAY_NAME").map(|v| v.to_string()).unwrap_or_else(|_| "-".to_string());
        let description = ctx.env.var("RELAY_DESCRIPTION").map(|v| v.to_string()).unwrap_or_else(|_| "A stateless public NWC relay running on Cloudflare Workers.".to_string());
        let pubkey = ctx.env.var("RELAY_PUBKEY").map(|v| v.to_string()).unwrap_or_else(|_| "".to_string());
        let contact = ctx.env.var("RELAY_CONTACT").map(|v| v.to_string()).unwrap_or_else(|_| "".to_string());
        let software = ctx.env.var("RELAY_SOFTWARE").map(|v| v.to_string()).unwrap_or_else(|_| "".to_string());
        let version = ctx.env.var("RELAY_VERSION").map(|v| v.to_string()).unwrap_or_else(|_| "main".to_string());

        let info = serde_json::json!({
            "name": name,
            "description": description,
            "pubkey": pubkey,
            "contact": contact,
            "supported_nips": [1, 11, 47],
            "software": software,
            "version": version
        });

        let headers = Headers::new();
        headers.set("Content-Type", "application/nostr+json")?;
        headers.set("Access-Control-Allow-Origin", "*")?;

        return Ok(Response::from_json(&info)?.with_headers(headers));
    }

    Response::ok("nwc-edge-relay: Nostr Wallet Connect Relay")
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
                    ws.serialize_attachment(&subscriptions)?;
                    let _ = ws.send_with_str(&RelayMessage::Eose(sub_id).to_json());
                }
                ClientMessage::Close(sub_id) => {
                    subscriptions.remove(&sub_id);
                    ws.serialize_attachment(&subscriptions)?;
                }
            }
        }
        Ok(())
    }

    async fn websocket_close(&self, _ws: WebSocket, _code: usize, _reason: String, _was_clean: bool) -> Result<()> {
        Ok(())
    }
}
