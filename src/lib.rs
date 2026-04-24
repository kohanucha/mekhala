use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use worker::*;
use futures_util::StreamExt;

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

fn handle_get_info(req: Request, _ctx: RouteContext<()>) -> Result<Response> {
    if req.headers().get("Accept")?.as_deref() == Some("application/nostr+json") {
        let info = serde_json::json!({
            "name": "nwc-worker",
            "description": "A stateless public NWC relay running on Cloudflare Workers.",
            "pubkey": "6e468422c0020d52899347d4e3415c464c483a3d53716d6100c5c3b9b46e3d00",
            "contact": "https://github.com/kohanucha/nwc-worker",
            "supported_nips": [1, 11, 47],
            "software": "https://github.com/kohanucha/nwc-worker",
            "version": "0.1.0"
        });

        let headers = Headers::new();
        headers.set("Content-Type", "application/nostr+json")?;
        headers.set("Access-Control-Allow-Origin", "*")?;

        return Ok(Response::from_json(&info)?.with_headers(headers));
    }

    Response::ok("nwc-worker: Nostr Wallet Connect Relay")
}

type Sessions = Arc<Mutex<HashMap<String, (WebSocket, HashMap<String, Vec<Filter>>)>>>;

#[durable_object]
pub struct NwcRelay {
    _state: State,
    _env: Env,
    sessions: Sessions,
}

impl DurableObject for NwcRelay {
    fn new(state: State, env: Env) -> Self {
        Self {
            _state: state,
            _env: env,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let upgrade_query = req.headers().get("Upgrade")?;
        if upgrade_query.as_deref() != Some("websocket") {
            return Response::error("Expected Upgrade: websocket", 426);
        }

        let WebSocketPair { client, server } = WebSocketPair::new()?;
        server.accept()?;

        // Generate a random session ID using crypto random
        let mut id_bytes = [0u8; 16];
        if getrandom::getrandom(&mut id_bytes).is_err() {
            return Response::error("Failed to generate session ID", 500);
        }
        let session_id = hex::encode(id_bytes);

        self.sessions.lock().unwrap().insert(session_id.clone(), (server.clone(), HashMap::new()));

        let sessions = self.sessions.clone();
        
        wasm_bindgen_futures::spawn_local(async move {
            let mut event_stream = server.events().expect("Failed to get events stream");

            while let Some(event) = event_stream.next().await {
                match event.expect("WebSocket error") {
                    WebsocketEvent::Message(msg) => {
                        if let Some(text) = msg.text() {
                            if text.len() > 65536 {
                                let _ = server.send_with_str(&RelayMessage::Notice("error: message too large".into()).to_json());
                                continue;
                            }

                            let client_msg = match ClientMessage::from_json(&text) {
                                Some(m) => m,
                                None => continue, // Ignore unrecognized messages
                            };

                            match client_msg {
                                ClientMessage::Event(event) => {
                                    if !event.verify() {
                                        let _ = server.send_with_str(&RelayMessage::Ok(event.id, false, "invalid: signature verification failed".into()).to_json());
                                        continue;
                                    }

                                    let _ = server.send_with_str(&RelayMessage::Ok(event.id.clone(), true, "".into()).to_json());

                                    // Broadcast to subscribers
                                    let sessions_lock = sessions.lock().unwrap();
                                    for (_, (ws, subs)) in sessions_lock.iter() {
                                        for (sub_id, filters) in subs.iter() {
                                            if filters.iter().any(|f| f.matches(&event)) {
                                                let _ = ws.send_with_str(&RelayMessage::Event(sub_id.clone(), event.clone()).to_json());
                                            }
                                        }
                                    }
                                }
                                ClientMessage::Req(sub_id, filters) => {
                                    if let Some((_, subs)) = sessions.lock().unwrap().get_mut(&session_id) {
                                        subs.insert(sub_id.clone(), filters);
                                    }
                                    let _ = server.send_with_str(&RelayMessage::Eose(sub_id).to_json());
                                }
                                ClientMessage::Close(sub_id) => {
                                    if let Some((_, subs)) = sessions.lock().unwrap().get_mut(&session_id) {
                                        subs.remove(&sub_id);
                                    }
                                }
                            }
                        }
                    }
                    WebsocketEvent::Close(_) => {
                        sessions.lock().unwrap().remove(&session_id);
                        break;
                    }
                }
            }
        });

        Response::from_websocket(client)
    }
}
