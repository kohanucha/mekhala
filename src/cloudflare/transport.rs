use std::collections::{HashMap, HashSet};
use std::cell::RefCell;
use futures::channel::oneshot;
use futures_util::FutureExt;
use worker::*;
use wasm_bindgen::{JsValue, JsCast};
use crate::nostr::engine::{NostrEngine, EngineResponse};
use crate::cloudflare::headers::create_cors_response;
use crate::cloudflare::HibernationState;

#[durable_object]
pub struct CloudflareTransport {
    state: State,
    env: Env,
    engine: RefCell<NostrEngine>,
    id_map: RefCell<Vec<(WebSocket, u32)>>,
    internal_connections: RefCell<HashMap<u32, (Option<oneshot::Sender<String>>, Option<oneshot::Receiver<String>>)>>,
}

impl DurableObject for CloudflareTransport {
    fn new(state: State, env: Env) -> Self {
        let engine = NostrEngine::new();

        Self {
            state,
            env,
            engine: RefCell::new(engine),
            id_map: RefCell::new(Vec::new()),
            internal_connections: RefCell::new(HashMap::new()),
        }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path();

        // Handle delegated LN Address callback
        if path.starts_with("/lnaddress/") && path.ends_with("/callback") {
            let username = path.strip_prefix("/lnaddress/").and_then(|s| s.strip_suffix("/callback")).unwrap_or("");
            return crate::lnaddress::handle_lnaddress_callback(req, &self.env, username, self).await;
        }

        self.accept_new_connection().await
    }

    async fn websocket_message(&self, websocket: WebSocket, message: WebSocketIncomingMessage) -> Result<()> {
        if let WebSocketIncomingMessage::String(text) = message {
            if text.len() > 65536 {
                let _ = websocket.send_with_str(&self.engine.borrow().error_message("message too large"));
                return Ok(());
            }
            let connection_id = self.wake_up(&websocket).await.ok_or_else(|| Error::from("Connection not found"))?;

            // Pre-flight Loading for EVENT messages to support O(1) routing
            self.load_recipients(&text).await;
            
            let responses = self.engine.borrow_mut().on_message(connection_id, &text);
            self.process_responses(responses).await?;

            Ok(())
        } else {
            let _ = websocket.send_with_str(&self.engine.borrow().error_message("binary not supported"));
            Ok(())
        }
    }

    async fn websocket_close(&self, ws: WebSocket, _: usize, _: String, _: bool) -> Result<()> {
        self.handle_disconnect(&ws).await
    }

    async fn websocket_error(&self, ws: WebSocket, _: Error) -> Result<()> {
        self.handle_disconnect(&ws).await
    }
}

#[async_trait::async_trait(?Send)]
impl crate::common::InternalTransport for CloudflareTransport {
    async fn load_connection(&self, pubkey: &str) -> Result<Option<u32>> {
        let storage = self.state.storage();
        let key = format!("pk:{}", pubkey);
        let id: Option<u32> = storage.get::<u32>(&key).await.unwrap_or(None);

        if let Some(id) = id {
            let tag = format!("id:{}", id);
            for ws in self.get_websockets_with_tag(&tag) {
                if let Some(actual_id) = self.wake_up(&ws).await {
                    if actual_id == id {
                        return Ok(Some(id));
                    }
                }
            }
            
            // Fallback: search all sockets if tagged one failed to wake up
            for ws in self.state.get_websockets() {
                if let Some(actual_id) = self.wake_up(&ws).await {
                    if actual_id == id {
                        return Ok(Some(id));
                    }
                }
            }
        }
        Ok(None)
    }

    async fn get_wallet_info(&self, pubkey: &str) -> Option<crate::nostr::WalletInfo> {
        let _ = <Self as crate::common::InternalTransport>::load_connection(self, pubkey).await.ok();
        Some(self.engine.borrow().get_wallet_info(pubkey))
    }

    async fn create_connection(&self) -> Result<u32> {
        let id = self.generate_unique_id().await;
        let (tx, rx) = oneshot::channel();
        self.internal_connections.borrow_mut().insert(id, (Some(tx), Some(rx)));
        Ok(id)
    }

    async fn send_message(&self, id: u32, message: String) -> Result<()> {
        // Pre-flight Loading for EVENT messages to support O(1) routing
        self.load_recipients(&message).await;

        let responses = self.engine.borrow_mut().on_message(id, &message);
        self.process_responses(responses).await?;
        Ok(())
    }

    async fn receive_message(&self, id: u32) -> Result<String> {
        let rx = {
            let mut channels = self.internal_connections.borrow_mut();
            let entry = channels.get_mut(&id).ok_or_else(|| Error::from("Connection not found"))?;
            entry.1.take().ok_or_else(|| Error::from("Already receiving or channel closed"))?
        };

        let delay = worker::Delay::from(std::time::Duration::from_secs(10)).fuse();
        futures_util::pin_mut!(rx, delay);

        match futures_util::future::select(rx, delay).await {
            futures_util::future::Either::Left((Ok(response), _)) => Ok(response),
            _ => Err(Error::from("Dispatch timeout")),
        }
    }

    async fn close_connection(&self, id: u32) -> Result<()> {
        let responses = self.engine.borrow_mut().on_disconnect(id);
        self.process_responses(responses).await?;
        self.internal_connections.borrow_mut().remove(&id);
        Ok(())
    }
}

impl CloudflareTransport {
    async fn process_responses(&self, responses: Vec<EngineResponse>) -> Result<()> {
        for resp in responses {
            match resp {
                EngineResponse::Send { connection_id, message } => {
                    if let Some((ws, _)) = self.id_map.borrow().iter().find(|(_, i)| *i == connection_id) {
                        let _ = ws.send_with_str(&message);
                    }
                }
                EngineResponse::Signal { connection_id, message } => {
                    let mut channels = self.internal_connections.borrow_mut();
                    if let Some(entry) = channels.get_mut(&connection_id) {
                        if let Some(sender) = entry.0.take() {
                            let _ = sender.send(message);
                        }
                    }
                }
                EngineResponse::StoreState { connection_id, connection_data } => {
                    self.sync_to_storage_data(connection_id, connection_data).await;
                }
            }
        }
        Ok(())
    }

    async fn sync_to_storage_data(&self, connection_id: u32, state: serde_json::Value) {
        let storage = self.state.storage();
        let key = format!("conn:{}", connection_id);
        let _ = storage.put(&key, state.clone()).await;

        // Update pubkey index
        if let Some(registry) = state.get("registry") {
            if let Some(subs) = registry.get("subscriptions") {
                if let Ok(subs_map) = serde_json::from_value::<HashMap<String, Vec<crate::nostr::Filter>>>(subs.clone()) {
                    let mut pks = HashSet::new();
                    for filters in subs_map.values() {
                        for filter in filters {
                            for pk in filter.pubkeys() {
                                pks.insert(pk);
                            }
                        }
                    }

                    for pk in pks {
                        let pk_key = format!("pk:{}", pk);
                        // Store only the latest connection ID for this pubkey (Last-In-Wins)
                        // Note: Connection ID 0 (System) also gets indexed so bridge can route to it if needed
                        let _ = storage.put(&pk_key, connection_id).await;
                    }
                }
            }
        }
    }

    fn get_websockets_with_tag(&self, tag: &str) -> Vec<WebSocket> {
        let state_js: &JsValue = unsafe { std::mem::transmute(&self.state) };
        let state_ext: &crate::cloudflare::hibernation::DurableObjectStateExt = state_js.unchecked_ref();
        let js_array = state_ext.get_websockets_raw(Some(tag));
        let mut result = Vec::new();
        for i in 0..js_array.length() {
            let ws_js = js_array.get(i);
            // Convert JsValue to web_sys::WebSocket, then to worker::WebSocket
            let web_sys_ws: worker::web_sys::WebSocket = ws_js.unchecked_into();
            let ws: WebSocket = web_sys_ws.into();
            result.push(ws);
        }
        result
    }

    async fn wake_up(&self, ws: &WebSocket) -> Option<u32> {
        if let Some(id) = self.get_id(ws) {
            return Some(id);
        }
        
        // Try to recover from tags
        let tags = self.state.get_tags(ws);
        let id_tag = tags.iter().find(|t| t.starts_with("id:"))?;
        let id: u32 = id_tag.strip_prefix("id:")?.parse().ok()?;

        // Hydrate from storage
        let storage = self.state.storage();
        let key = format!("conn:{}", id);
        if let Ok(Some(state)) = storage.get::<serde_json::Value>(&key).await {
            self.engine.borrow_mut().import_state(id, state);
            self.id_map.borrow_mut().push((ws.clone(), id));
            return Some(id);
        }
        None
    }

    async fn load_recipients(&self, text: &str) {
        if let Some(target_pks) = self.engine.borrow().get_target_pubkeys(text) {
            let storage = self.state.storage();
            // Load recipients into engine memory
            for pk in target_pks {
                let key = format!("pk:{}", pk);
                if let Ok(Some(rid)) = storage.get::<u32>(&key).await {
                    let tag = format!("id:{}", rid);
                    for rws in self.get_websockets_with_tag(&tag) {
                        let _ = self.wake_up(&rws).await;
                    }
                }
            }
        }
    }

    fn get_id(&self, ws: &WebSocket) -> Option<u32> {
        self.id_map.borrow().iter().find(|(w, _)| ws_eq(w, ws)).map(|(_, id)| *id)
    }

    async fn generate_unique_id(&self) -> u32 {
        // Use timestamp-based counter to ensure strictly increasing IDs for "Last-In-Wins" logic.
        // Even if the DO restarts, the timestamp will ensure we stay ahead of old IDs.
        let storage = self.state.storage();
        let mut counter = storage.get::<u32>("id_counter").await.ok().flatten().unwrap_or(0);
        let now = crate::util::now() as u32;
        
        if counter < now {
            counter = now;
        }
        counter += 1;
        
        let _ = storage.put("id_counter", counter).await;
        counter
    }

    async fn accept_new_connection(&self) -> Result<Response> {
        let WebSocketPair { client, server } = WebSocketPair::new()?;
        self.state.accept_web_socket(&server);

        let connection_id = self.generate_unique_id().await;
        let responses = self.engine.borrow_mut().on_connect(connection_id);
        self.id_map.borrow_mut().push((server.clone(), connection_id));
        let _ = self.state.set_tags(&server, vec![format!("id:{}", connection_id)]);

        self.process_responses(responses).await?;

        Ok(Response::from_websocket(client)?)
    }

    async fn handle_disconnect(&self, ws: &WebSocket) -> Result<()> {
        if let Some(id) = self.wake_up(ws).await {
            let responses = self.engine.borrow_mut().on_disconnect(id);
            self.process_responses(responses).await?;
            self.id_map.borrow_mut().retain(|(_, i)| *i != id);
            
            // Clean up storage
            let storage = self.state.storage();
            let _ = storage.delete(&format!("conn:{}", id)).await;
            // Note: cleaning up pk index is more complex (scan-intensive),
            // but for personal relay we can let it be or implement gradual cleanup.
        }
        Ok(())
    }
}

fn ws_eq(a: &WebSocket, b: &WebSocket) -> bool {
    js_sys::Object::is(a.as_ref(), b.as_ref())
}

pub async fn connect(req: Request, env: &Env) -> Result<Response> {
    crate::cloudflare::get_durable_stub(env)?.fetch_with_request(req).await
}

pub fn create_response(info: serde_json::Value, content_type: &str) -> Result<Response> {
    let mut response = create_cors_response(Response::from_json(&info)?)?;
    response.headers_mut().set("Content-Type", content_type)?;
    Ok(response)
}
