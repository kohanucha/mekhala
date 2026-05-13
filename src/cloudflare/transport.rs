use std::collections::HashMap;
use std::cell::RefCell;
use futures::channel::oneshot;
use futures::lock::Mutex;
use worker::*;
use wasm_bindgen::{JsValue, JsCast};
use crate::nostr::engine::{NostrEngine, EngineResponse, MessageFlags};
use crate::nostr::wallet_registry::{PersistentWalletRegistry, WalletRegistry, Storage};
use crate::cloudflare::headers::create_cors_response;
use crate::cloudflare::HibernationState;

pub struct CloudflareStorage {
    storage: worker::Storage,
}

#[async_trait::async_trait(?Send)]
impl Storage for CloudflareStorage {
    async fn put(&self, key: &str, value: serde_json::Value) {
        let _ = self.storage.put(key, value).await;
    }
    async fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.storage.get(key).await.ok().flatten()
    }
    async fn delete(&self, key: &str) {
        let _ = self.storage.delete(key).await;
    }
}

#[durable_object]
pub struct CloudflareTransport {
    state: State,
    env: Env,
    engine: Mutex<NostrEngine<PersistentWalletRegistry<CloudflareStorage>>>,
    id_map: RefCell<Vec<(WebSocket, u32)>>,
    internal_map: RefCell<HashMap<u32, oneshot::Sender<String>>>,
}

impl DurableObject for CloudflareTransport {
    fn new(state: State, env: Env) -> Self {
        let storage = CloudflareStorage { storage: state.storage() };
        let registry = PersistentWalletRegistry::new(storage);
        let engine = NostrEngine { registry };

        Self {
            state,
            env,
            engine: Mutex::new(engine),
            id_map: RefCell::new(Vec::new()),
            internal_map: RefCell::new(HashMap::new()),
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
            let mut engine = self.engine.lock().await;

            if text.len() > 65536 {
                let _ = websocket.send_with_str(&engine.error_message("message too large"));
                return Ok(());
            }
            let connection_id = self.wake_up_with_engine(&websocket, &mut engine).await.ok_or_else(|| Error::from("Connection not found"))?;

            // Pre-flight Loading for EVENT messages to support O(1) routing
            self.load_recipients_with_engine(&mut engine, &text).await;
            
            let responses = engine.on_message(connection_id, &text, MessageFlags::default()).await;
            self.process_responses(responses).await?;

            Ok(())
        } else {
            let engine = self.engine.lock().await;
            let _ = websocket.send_with_str(&engine.error_message("binary not supported"));
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
    async fn get_wallet_info(&self, pubkey: &str) -> Option<crate::nostr::WalletInfo> {
        let mut engine = self.engine.lock().await;
        let _ = self.load_connection_with_engine(pubkey, &mut engine).await.ok();
        Some(engine.get_wallet_info(pubkey))
    }

    async fn send_message(&self, id: u32, message: String, sender: oneshot::Sender<String>) -> Result<()> {
        self.internal_map.borrow_mut().insert(id, sender);

        let mut engine = self.engine.lock().await;

        // Pre-flight Loading for EVENT messages to support O(1) routing
        self.load_recipients_with_engine(&mut engine, &message).await;

        let responses = engine.on_message(id, &message, MessageFlags { is_internal: true }).await;
        self.process_responses(responses).await?;
        Ok(())
    }

    async fn generate_id(&self) -> u32 {
        self.generate_unique_id().await
    }

    async fn close_connection(&self, id: u32) -> Result<()> {
        let mut engine = self.engine.lock().await;
        let responses = engine.on_disconnect(id).await;
        self.process_responses(responses).await?;
        self.internal_map.borrow_mut().remove(&id);
        Ok(())
    }
}

impl CloudflareTransport {
    async fn load_connection_with_engine(&self, pubkey: &str, engine: &mut NostrEngine<PersistentWalletRegistry<CloudflareStorage>>) -> Result<Option<u32>> {
        if let Some(id) = engine.registry.load_by_pubkey(pubkey).await {
            let tag = format!("id:{}", id);
            for ws in self.get_websockets_with_tag(&tag) {
                if let Some(actual_id) = self.wake_up_with_engine(&ws, engine).await {
                    if actual_id == id {
                        return Ok(Some(id));
                    }
                }
            }

            // Fallback: search all sockets if tagged one failed to wake up
            for ws in self.state.get_websockets() {
                if let Some(actual_id) = self.wake_up_with_engine(&ws, engine).await {
                    if actual_id == id {
                        return Ok(Some(id));
                    }
                }
            }
            return Ok(Some(id));
        }
        Ok(None)
    }

    async fn process_responses(&self, responses: Vec<EngineResponse>) -> Result<()> {
        for resp in responses {
            match resp {
                EngineResponse::Send { connection_id, message } => {
                    // 1. Try internal routing first
                    let internal_sender = self.internal_map.borrow_mut().remove(&connection_id);
                    if let Some(sender) = internal_sender {
                        let _ = sender.send(message);
                        continue;
                    }

                    // 2. Fallback to WebSocket routing
                    if let Some((ws, _)) = self.id_map.borrow().iter().find(|(_, i)| *i == connection_id) {
                        let _ = ws.send_with_str(&message);
                    }
                }
            }
        }
        Ok(())
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

    async fn wake_up_with_engine(&self, ws: &WebSocket, engine: &mut NostrEngine<PersistentWalletRegistry<CloudflareStorage>>) -> Option<u32> {
        if let Some(id) = self.get_id(ws) {
            return Some(id);
        }
        
        // Try to recover from tags
        let tags = self.state.get_tags(ws);
        let id_tag = tags.iter().find(|t| t.starts_with("id:"))?;
        let id: u32 = id_tag.strip_prefix("id:")?.parse().ok()?;

        if engine.registry.load(id).await {
            self.id_map.borrow_mut().push((ws.clone(), id));
            return Some(id);
        }
        None
    }

    async fn load_recipients_with_engine(&self, engine: &mut NostrEngine<PersistentWalletRegistry<CloudflareStorage>>, text: &str) {
        if let Some(target_pks) = engine.get_target_pubkeys(text) {
            // Load recipients into engine memory
            for pk in target_pks {
                if let Some(rid) = engine.registry.load_by_pubkey(&pk).await {
                    let tag = format!("id:{}", rid);
                    for rws in self.get_websockets_with_tag(&tag) {
                        let _ = self.wake_up_with_engine(&rws, engine).await;
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

        let mut engine = self.engine.lock().await;
        let connection_id = self.generate_unique_id().await;
        let responses = engine.on_connect(connection_id).await;
        self.id_map.borrow_mut().push((server.clone(), connection_id));
        let _ = self.state.set_tags(&server, vec![format!("id:{}", connection_id)]);

        self.process_responses(responses).await?;

        Ok(Response::from_websocket(client)?)
    }

    async fn handle_disconnect(&self, ws: &WebSocket) -> Result<()> {
        let mut engine = self.engine.lock().await;
        if let Some(id) = self.wake_up_with_engine(ws, &mut engine).await {
            let responses = engine.on_disconnect(id).await;
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
