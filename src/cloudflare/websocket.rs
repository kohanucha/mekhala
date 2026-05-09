use std::collections::{HashMap, HashSet};
use std::cell::RefCell;
use futures::channel::oneshot;
use futures_util::FutureExt;
use serde_json::Value;
use worker::*;
use wasm_bindgen::{JsValue, JsCast};
use crate::util::engine::{Engine, EngineResponse};
use crate::cloudflare::{apply_security_headers, HibernationState};

#[durable_object]
pub struct Websocket {
    state: State,
    env: Env,
    engine: RefCell<Box<dyn Engine>>,
    id_map: RefCell<Vec<(WebSocket, u32)>>,
    dispatch_channels: RefCell<HashMap<String, oneshot::Sender<String>>>,
}

impl DurableObject for Websocket {
    fn new(state: State, env: Env) -> Self {
        let engine = crate::nostr::create_engine();

        Self {
            state,
            env,
            engine: RefCell::new(engine),
            id_map: RefCell::new(Vec::new()),
            dispatch_channels: RefCell::new(HashMap::new()),
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

        if path.starts_with("/internal/dispatch/") && req.method() == Method::Post {
            let pubkey = path.strip_prefix("/internal/dispatch/").unwrap_or("");
            return crate::lnaddress::wallet_connector::handle_internal_dispatch(self, pubkey, req).await;
        }

        if let Some(info) = self.engine.borrow().get_info(path) {
            return apply_security_headers(Response::from_json(&info)?);
        }

        self.accept_new_connection().await
    }

    async fn websocket_message(&self, ws: WebSocket, message: WebSocketIncomingMessage) -> Result<()> {
        if let WebSocketIncomingMessage::String(text) = message {
            if text.len() > 65536 {
                let _ = ws.send_with_str(&self.engine.borrow().error_message("message too large"));
                return Ok(());
            }
            let connection_id = self.ensure_loaded(&ws).await.ok_or_else(|| Error::from("Connection not found"))?;

            // Pre-flight Loading for EVENT messages to support O(1) routing
            self.load_recipients(&text).await;
            
            let responses = self.engine.borrow_mut().on_message(connection_id, &text);
            self.process_responses(responses)?;

            // Sync updated state to storage
            self.sync_to_storage(connection_id).await;

            Ok(())
        } else {
            let _ = ws.send_with_str(&self.engine.borrow().error_message("binary not supported"));
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
impl crate::cloudflare::RelayTransport for Websocket {
    fn inject_message(&self, id: u32, msg: &str) -> Result<()> {
        let responses = self.engine.borrow_mut().on_message(id, msg);
        self.process_responses(responses)
    }

    fn send_raw(&self, id: u32, msg: &str) -> Result<()> {
        if let Some((ws, _)) = self.id_map.borrow().iter().find(|(_, i)| *i == id) {
            let _ = ws.send_with_str(msg);
        }
        Ok(())
    }

    async fn load_connections(&self, pubkey: &str) -> Result<Vec<u32>> {
        let storage = self.state.storage();
        let key = format!("pk:{}", pubkey);
        let ids: Vec<u32> = storage.get::<Vec<u32>>(&key).await.unwrap_or(None).unwrap_or_default();

        worker::console_log!("load_connections for {}: found storage IDs {:?}", pubkey, ids);

        let mut loaded_ids = Vec::new();
        for id in &ids {
            let tag = format!("id:{}", id);
            let mut found_by_tag = false;
            for ws in self.get_websockets_with_tag(&tag) {
                if let Some(actual_id) = self.ensure_loaded(&ws).await {
                    found_by_tag = true;
                    if !loaded_ids.contains(&actual_id) {
                        loaded_ids.push(actual_id);
                    }
                }
            }
            
            if !found_by_tag {
                for ws in self.state.get_websockets() {
                    if let Some(actual_id) = self.ensure_loaded(&ws).await {
                        if actual_id == *id && !loaded_ids.contains(&actual_id) {
                            loaded_ids.push(actual_id);
                        }
                    }
                }
            }
        }
        Ok(loaded_ids)
    }

    fn register_dispatch(&self, sub_id: String, sender: futures::channel::oneshot::Sender<String>) {
        self.dispatch_channels.borrow_mut().insert(sub_id, sender);
    }

    async fn get_info(&self, path: &str) -> Option<Value> {
        self.engine.borrow().get_info(path)
    }

    async fn dispatch_nwc(&self, target_pubkey: &str, event: crate::nostr::Event) -> Result<String> {
        let loaded_ids = self.load_connections(target_pubkey).await?;

        if loaded_ids.is_empty() {
            return Err(Error::from("Wallet not connected"));
        }

        let (tx, rx) = oneshot::channel();
        let sub_id = format!("disp_{}_{}", target_pubkey.get(..8).unwrap_or(target_pubkey), worker::js_sys::Math::random().to_string().get(2..10).unwrap_or(""));
        
        self.register_dispatch(sub_id.clone(), tx);

        // 1. Inject REQ to listen for response
        let primary_id = loaded_ids[0];
        let req_msg = serde_json::json!([
            "REQ",
            sub_id,
            {
                "kinds": [crate::nostr::nip_47::KIND_NWC_RESPONSE],
                "#p": [event.pubkey], 
                "#e": [event.id]
            }
        ]).to_string();
        self.inject_message(primary_id, &req_msg)?;

        // 2. Inject EVENT (the request) - engine routes to all active subscribers
        let event_msg = serde_json::json!(["EVENT", event]).to_string();
        self.inject_message(primary_id, &event_msg)?;

        // Wait for response with timeout
        let rx_fuse = rx.fuse();
        let delay = worker::Delay::from(std::time::Duration::from_secs(10)).fuse();
        
        futures_util::pin_mut!(rx_fuse, delay);

        match futures_util::future::select(rx_fuse, delay).await {
            futures_util::future::Either::Left((Ok(response), _)) => {
                Ok(response)
            }
            _ => {
                Err(Error::from("Dispatch timeout"))
            }
        }
    }
}

impl Websocket {
    fn process_responses(&self, responses: Vec<EngineResponse>) -> Result<()> {
        for resp in responses {
            if resp.messages.is_empty() {
                continue;
            }

            let mut intercepted = false;
            if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&resp.messages) {
                if arr.len() >= 2 {
                    let msg_type = arr[0].as_str().unwrap_or("");
                    let sub_id = arr[1].as_str().unwrap_or("");

                    // Handle Internal Dispatch interception
                    if !sub_id.is_empty() && (msg_type == "EVENT" || msg_type == "CLOSED") {
                        worker::console_log!("Intercepting response for sub_id: {}", sub_id);
                        if let Some(sender) = self.dispatch_channels.borrow_mut().remove(sub_id) {
                            let _ = sender.send(resp.messages.clone());
                            intercepted = true;
                            worker::console_log!("Response sent to dispatch channel.");
                        }
                    }
                }
            }

            if !intercepted {
                for conn_id in resp.connection_ids {
                    if let Some((ws, _)) = self.id_map.borrow().iter().find(|(_, i)| *i == conn_id) {
                        let _ = ws.send_with_str(&resp.messages);
                    }
                }
            }
        }
        Ok(())
    }

    async fn sync_to_storage(&self, id: u32) {
        if let Some(state) = self.engine.borrow().export_state(id) {
            let storage = self.state.storage();
            let key = format!("conn:{}", id);
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
                            let mut ids: Vec<u32> = storage.get::<Vec<u32>>(&pk_key).await.unwrap_or(None).unwrap_or_default();
                            if !ids.contains(&id) {
                                ids.push(id);
                                let _ = storage.put(&pk_key, ids).await;
                            }
                        }
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

    async fn ensure_loaded(&self, ws: &WebSocket) -> Option<u32> {
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
                let ids: Vec<u32> = storage.get::<Vec<u32>>(&key).await.unwrap_or(None).unwrap_or_default();
                for rid in ids {
                    let tag = format!("id:{}", rid);
                    for rws in self.get_websockets_with_tag(&tag) {
                        let _ = self.ensure_loaded(&rws).await;
                    }
                }
            }
        }
    }

    fn get_id(&self, ws: &WebSocket) -> Option<u32> {
        self.id_map.borrow().iter().find(|(w, _)| ws_eq(w, ws)).map(|(_, id)| *id)
    }

    fn generate_unique_id(&self) -> u32 {
        let mut new_id = rand::random::<u32>();
        while self.id_map.borrow().iter().any(|(_, id)| *id == new_id) || new_id == 0 {
            new_id = rand::random::<u32>();
        }
        new_id
    }

    async fn accept_new_connection(&self) -> Result<Response> {
        let WebSocketPair { client, server } = WebSocketPair::new()?;
        self.state.accept_web_socket(&server);

        let connection_id = self.generate_unique_id();
        let responses = self.engine.borrow_mut().on_connect(connection_id);
        self.id_map.borrow_mut().push((server.clone(), connection_id));
        let _ = self.state.set_tags(&server, vec![format!("id:{}", connection_id)]);

        self.process_responses(responses)?;

        Ok(Response::from_websocket(client)?)
    }

    async fn handle_disconnect(&self, ws: &WebSocket) -> Result<()> {
        if let Some(id) = self.ensure_loaded(ws).await {
            let responses = self.engine.borrow_mut().on_disconnect(id);
            self.process_responses(responses)?;
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
