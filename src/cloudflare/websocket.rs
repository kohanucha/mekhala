use std::cell::UnsafeCell;
use std::collections::HashMap;
use futures::channel::oneshot;
use futures_util::future::{select, Either};
use futures_util::{pin_mut, FutureExt};
use serde_json::Value;
use worker::*;
use crate::cloudflare::{accept_connection, HibernationState};
use crate::util::engine::{Engine, EngineAction};
use crate::util::transport::SyncTransport;
use crate::cloudflare::apply_security_headers;

#[durable_object]
pub struct Websocket {
    state: State,
    // Using UnsafeCell for the engine to allow the engine to call back into the transport (self)
    // while the engine itself is "being called". This is safe because Durable Objects are single-threaded
    // and our callbacks don't perform re-entrant mutations on the engine.
    engine: UnsafeCell<Box<dyn Engine>>,
    id_map: UnsafeCell<Vec<(WebSocket, u32)>>,
    dispatch_channels: UnsafeCell<HashMap<String, oneshot::Sender<String>>>,
}

impl DurableObject for Websocket {
    fn new(state: State, _env: Env) -> Self {
        let mut engine = crate::nostr::create_engine();
        let mut id_map = Vec::new();

        for ws in state.get_websockets() {
            if let Ok(Some(blob)) = ws.deserialize_attachment::<Vec<u8>>() {
                if blob.len() >= 4 {
                    let id = u32::from_le_bytes(blob[0..4].try_into().unwrap_or([0; 4]));
                    let engine_state = blob[4..].to_vec();
                    // Restoration connect - usually doesn't need commit as state is already in attachment
                    engine.on_connect(&NoopTransport, id, Some(engine_state));
                    id_map.push((ws, id));
                }
            }
        }

        Self {
            state,
            engine: UnsafeCell::new(engine),
            id_map: UnsafeCell::new(id_map),
            dispatch_channels: UnsafeCell::new(HashMap::new()),
        }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path();

        if path.starts_with("/internal/dispatch/") && req.method() == Method::Post {
            let pubkey = path.strip_prefix("/internal/dispatch/").unwrap_or("");
            return self.handle_dispatch(pubkey, req).await;
        }

        let engine = unsafe { &*self.engine.get() };
        if let Some(info) = engine.get_info(path) {
            return apply_security_headers(Response::from_json(&info)?);
        }

        self.accept_new_connection()
    }

    async fn websocket_message(&self, ws: WebSocket, message: WebSocketIncomingMessage) -> Result<()> {
        let engine = unsafe { &mut *self.engine.get() };
        if let WebSocketIncomingMessage::String(text) = message {
            if text.len() > 65536 {
                let _ = ws.send_with_str(&engine.error_message("message too large"));
                return Ok(());
            }
            let id = self.get_id(&ws).ok_or_else(|| Error::from("Connection not found"))?;
            
            let action = engine.on_message(self, id, &text);
            
            if action == EngineAction::Commit {
                self.sync_connection_state(id);
            }
            Ok(())
        } else {
            let _ = ws.send_with_str(&engine.error_message("binary not supported"));
            Ok(())
        }
    }

    async fn websocket_close(&self, ws: WebSocket, _: usize, _: String, _: bool) -> Result<()> {
        self.handle_disconnect(&ws)
    }

    async fn websocket_error(&self, ws: WebSocket, _: Error) -> Result<()> {
        self.handle_disconnect(&ws)
    }
}

struct NoopTransport;
impl SyncTransport for NoopTransport {
    fn send(&self, _: u32, _: &str) {}
}

impl SyncTransport for Websocket {
    fn send(&self, id: u32, message: &str) {
        // Intercept for internal dispatch channels
        if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(message) {
            if arr.len() >= 2 {
                let msg_type = arr[0].as_str().unwrap_or("");
                let sub_id = arr[1].as_str().unwrap_or("");
                
                if !sub_id.is_empty() && (msg_type == "EVENT" || msg_type == "CLOSED") {
                    let dispatch_channels = unsafe { &mut *self.dispatch_channels.get() };
                    if let Some(sender) = dispatch_channels.remove(sub_id) {
                        let _ = sender.send(message.to_string());
                    }
                }
            }
        }

        let id_map = unsafe { &*self.id_map.get() };
        if let Some((ws, _)) = id_map.iter().find(|(_, i)| *i == id) {
            let _ = ws.send_with_str(message);
        }
    }
}

impl Websocket {
    async fn handle_dispatch(&self, pubkey: &str, mut req: Request) -> Result<Response> {
        let body_text = req.text().await?;
        let event: crate::nostr::Event = serde_json::from_str(&body_text)
            .map_err(|e| Error::from(format!("Invalid NWC Event: {}", e)))?;
        
        let engine = unsafe { &mut *self.engine.get() };
        let id = engine.get_connection_id(pubkey)
            .ok_or_else(|| Error::from("Wallet not connected"))?;

        let (tx, rx) = oneshot::channel();
        let sub_id = format!("disp_{}_{}", pubkey.get(..8).unwrap_or(pubkey), worker::js_sys::Math::random().to_string().get(2..10).unwrap_or(""));
        
        {
            let dispatch_channels = unsafe { &mut *self.dispatch_channels.get() };
            dispatch_channels.insert(sub_id.clone(), tx);
        }

        // 1. Inject REQ to listen for response
        let req_msg = serde_json::json!([
            "REQ",
            sub_id,
            {
                "kinds": [crate::nostr::nip_47::KIND_NWC_RESPONSE],
                "#p": [event.pubkey], // Index under bridge pubkey to ensure engine matches
                "#e": [event.id]
            }
        ]).to_string();
        engine.on_message(self, id, &req_msg);

        // 2. Inject EVENT (the request)
        let event_msg = serde_json::json!(["EVENT", event]).to_string();
        engine.on_message(self, id, &event_msg);

        // Wait for response with timeout
        let rx_fuse = rx.fuse();
        let delay = Delay::from(std::time::Duration::from_secs(10)).fuse();
        
        pin_mut!(rx_fuse, delay);

        match select(rx_fuse, delay).await {
            Either::Left((Ok(response), _)) => {
                apply_security_headers(Response::ok(response)?)
            }
            _ => {
                {
                    let dispatch_channels = unsafe { &mut *self.dispatch_channels.get() };
                    dispatch_channels.remove(&sub_id);
                }
                apply_security_headers(Response::error("Dispatch timeout", 504)?)
            }
        }
    }

    fn get_id(&self, ws: &WebSocket) -> Option<u32> {
        let id_map = unsafe { &*self.id_map.get() };
        id_map.iter().find(|(w, _)| ws_eq(w, ws)).map(|(_, id)| *id)
    }

    fn sync_connection_state(&self, id: u32) {
        let engine = unsafe { &*self.engine.get() };
        let id_map = unsafe { &*self.id_map.get() };
        
        if let Some((ws, _)) = id_map.iter().find(|(_, i)| *i == id) {
            // 1. Persistence (Push snapshot to WebSocket attachment)
            if let Some(snapshot) = engine.get_snapshot(id) {
                let mut full_blob = id.to_le_bytes().to_vec();
                full_blob.extend(snapshot);
                let _ = ws.serialize_attachment(&full_blob);
            }

            // 2. Hibernation Tags (Map interests to platform-specific tags)
            if let Some(interests) = engine.get_interests(id) {
                let mut tags = interests.pubkeys;
                for cap in interests.capabilities {
                    tags.push(format!("cap:{}", cap));
                }
                let _ = self.state.set_tags(ws, tags);
            }
        }
    }

    fn accept_new_connection(&self) -> Result<Response> {
        let engine = unsafe { &mut *self.engine.get() };
        let resp = accept_connection(&self.state, 100, engine.initial_state())?;
        
        let active_ws = self.state.get_websockets();
        let id_map = unsafe { &mut *self.id_map.get() };
        
        // Match the NEWEST websocket that isn't in our id_map yet.
        // During accept_connection, the new WS is added to the end of state.get_websockets().
        if let Some(new_ws) = active_ws.into_iter().find(|aw| !id_map.iter().any(|(w, _)| ws_eq(w, aw))) {
            let max_id = id_map.iter().map(|(_, id)| *id).max().unwrap_or(0);
            let new_id = max_id + 1;
            
            let action = engine.on_connect(self, new_id, None);
            id_map.push((new_ws, new_id));

            if action == EngineAction::Commit {
                self.sync_connection_state(new_id);
            }
        }

        Ok(resp)
    }

    fn handle_disconnect(&self, ws: &WebSocket) -> Result<()> {
        if let Some(id) = self.get_id(ws) {
            let engine = unsafe { &mut *self.engine.get() };
            let id_map = unsafe { &mut *self.id_map.get() };
            let action = engine.on_disconnect(self, id);
            
            if action == EngineAction::Commit {
                self.sync_connection_state(id);
            }
            
            id_map.retain(|(_, i)| *i != id);
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
