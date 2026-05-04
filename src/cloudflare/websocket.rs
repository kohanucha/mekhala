use std::cell::UnsafeCell;
use worker::*;
use crate::cloudflare::{accept_connection, HibernationState};
use crate::nostr::engine::NostrEngine;
use crate::util::engine::{Engine, GenericTransport};
use crate::cloudflare::apply_security_headers;

#[durable_object]
pub struct Websocket {
    state: State,
    // Using UnsafeCell for the engine to allow the engine to call back into the transport (self)
    // while the engine itself is "being called". This is safe because Durable Objects are single-threaded
    // and our callbacks don't perform re-entrant mutations on the engine.
    engine: UnsafeCell<Box<dyn Engine>>,
    id_map: UnsafeCell<Vec<(WebSocket, u32)>>,
}

impl DurableObject for Websocket {
    fn new(state: State, _env: Env) -> Self {
        let mut engine: Box<dyn Engine> = Box::new(NostrEngine::new());
        let mut id_map = Vec::new();

        for ws in state.get_websockets() {
            if let Ok(Some(blob)) = ws.deserialize_attachment::<Vec<u8>>() {
                if blob.len() >= 4 {
                    let id = u32::from_le_bytes(blob[0..4].try_into().unwrap_or([0;4]));
                    engine.on_connect(&NoopTransport, id, Some(blob));
                    id_map.push((ws, id));
                }
            }
        }

        Self {
            state,
            engine: UnsafeCell::new(engine),
            id_map: UnsafeCell::new(id_map),
        }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path();

        let engine = unsafe { &*self.engine.get() };
        if let Some(info) = engine.get_info(path) {
            return apply_security_headers(Response::from_json(&info)?);
        }

        self.accept_new_connection()
    }

    async fn websocket_message(&self, ws: WebSocket, message: WebSocketIncomingMessage) -> Result<()> {
        if let WebSocketIncomingMessage::String(text) = message {
            if text.len() > 65536 {
                let _ = ws.send_with_str(&crate::nostr::RelayMessage::Notice("message too large".to_string()).to_json());
                return Ok(());
            }
            let id = self.get_id(&ws).ok_or_else(|| Error::from("Connection not found"))?;
            
            let engine = unsafe { &mut *self.engine.get() };
            engine.on_message(self, id, &text);
            Ok(())
        } else {
            let _ = ws.send_with_str(&crate::nostr::RelayMessage::Notice("binary not supported".to_string()).to_json());
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
impl GenericTransport for NoopTransport {
    fn send(&self, _: u32, _: &str) {}
    fn persist(&self, _: u32, _: Vec<u8>) {}
    fn set_tags(&self, _: u32, _: Vec<String>) {}
}

impl GenericTransport for Websocket {
    fn send(&self, id: u32, message: &str) {
        let id_map = unsafe { &*self.id_map.get() };
        if let Some((ws, _)) = id_map.iter().find(|(_, i)| *i == id) {
            let _ = ws.send_with_str(message);
        }
    }

    fn persist(&self, id: u32, snapshot: Vec<u8>) {
        let id_map = unsafe { &*self.id_map.get() };
        if let Some((ws, _)) = id_map.iter().find(|(_, i)| *i == id) {
            let _ = ws.serialize_attachment(&snapshot);
        }
    }

    fn set_tags(&self, id: u32, tags: Vec<String>) {
        let id_map = unsafe { &*self.id_map.get() };
        if let Some((ws, _)) = id_map.iter().find(|(_, i)| *i == id) {
            self.state.set_tags(ws, tags);
        }
    }
}

impl Websocket {
    fn get_id(&self, ws: &WebSocket) -> Option<u32> {
        let id_map = unsafe { &*self.id_map.get() };
        id_map.iter().find(|(w, _)| ws_eq(w, ws)).map(|(_, id)| *id)
    }

    fn accept_new_connection(&self) -> Result<Response> {
        let resp = accept_connection(&self.state, 100)?;
        
        let active_ws = self.state.get_websockets();
        let id_map = unsafe { &mut *self.id_map.get() };
        let engine = unsafe { &mut *self.engine.get() };
        
        // Match the NEWEST websocket that isn't in our id_map yet.
        // During accept_connection, the new WS is added to the end of state.get_websockets().
        if let Some(new_ws) = active_ws.into_iter().find(|aw| !id_map.iter().any(|(w, _)| ws_eq(w, aw))) {
            let max_id = id_map.iter().map(|(_, id)| *id).max().unwrap_or(0);
            let new_id = max_id + 1;
            
            engine.on_connect(self, new_id, None);
            id_map.push((new_ws, new_id));
        }

        Ok(resp)
    }

    fn handle_disconnect(&self, ws: &WebSocket) -> Result<()> {
        if let Some(id) = self.get_id(ws) {
            let engine = unsafe { &mut *self.engine.get() };
            let id_map = unsafe { &mut *self.id_map.get() };
            engine.on_disconnect(self, id);
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
