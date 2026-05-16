use std::collections::HashMap;
use std::cell::Cell;
use futures::channel::oneshot;
use worker::*;
use wasm_bindgen::JsCast;
use crate::cloudflare::HibernationState;

pub enum Connection<W> {
    Active(W),
    Internal(oneshot::Sender<String>),
}

pub struct ConnectionRegistry {
    state: State,
    connections: HashMap<u32, Connection<WebSocket>>,
    current_id: Cell<u32>,
    id_limit: Cell<u32>,
}

impl ConnectionRegistry {
    pub fn new(state: State) -> Self {
        Self {
            state,
            connections: HashMap::new(),
            current_id: Cell::new(0),
            id_limit: Cell::new(0),
        }
    }

    pub fn add_active(&mut self, id: u32, ws: WebSocket) {
        self.connections.insert(id, Connection::Active(ws));
    }

    pub fn add_internal(&mut self, id: u32, sender: oneshot::Sender<String>) {
        self.connections.insert(id, Connection::Internal(sender));
    }

    pub fn register_active(&mut self, id: u32, ws: WebSocket) {
        let tag = format!("id:{}", id);
        self.state.set_tags(&ws, vec![tag]);
        self.add_active(id, ws);
    }

    pub fn identify(&self, ws: &WebSocket) -> Option<u32> {
        // 1. Check in-memory active connections
        for (id, conn) in &self.connections {
            if let Connection::Active(h) = conn {
                if js_sys::Object::is(h.as_ref(), ws.as_ref()) {
                    return Some(*id);
                }
            }
        }

        // 2. Fallback to tags (hibernation recovery)
        let tags = self.state.get_tags(ws);
        let id_tag = tags.iter().find(|t| t.starts_with("id:"))?;
        id_tag.strip_prefix("id:")?.parse().ok()
    }

    pub async fn next_id(&self) -> u32 {
        let current = self.current_id.get();
        let limit = self.id_limit.get();

        if current >= limit {
            // Buffer exhausted or not yet initialized
            let storage = self.state.storage();
            let last_limit = storage.get::<u32>("id_counter").await.ok().flatten().unwrap_or(0);
            
            // Sync with current time to ensure monotonicity after long periods of inactivity or DO move
            let now = crate::util::now() as u32;
            let start = std::cmp::max(last_limit, now);
            
            let new_limit = start + 1000;
            let _ = storage.put("id_counter", new_limit).await;
            
            self.current_id.set(start + 1);
            self.id_limit.set(new_limit);
            start + 1
        } else {
            let next = current + 1;
            self.current_id.set(next);
            next
        }
    }

    pub fn get_active(&self, id: u32) -> Option<WebSocket> {
        match self.connections.get(&id) {
            Some(Connection::Active(ws)) => Some(ws.clone()),
            _ => None,
        }
    }

    pub fn find_by_id(&self, id: u32) -> Option<WebSocket> {
        // 1. Check in-memory
        if let Some(ws) = self.get_active(id) {
            return Some(ws);
        }

        // 2. Search all sockets for the tag
        let tag = format!("id:{}", id);
        self.get_websockets_with_tag(&tag).into_iter().next()
    }

    fn get_websockets_with_tag(&self, tag: &str) -> Vec<WebSocket> {
        let state_js: &worker::wasm_bindgen::JsValue = unsafe { std::mem::transmute(&self.state) };
        let state_ext: &crate::cloudflare::hibernation::DurableObjectStateExt = state_js.unchecked_ref();
        let js_array = state_ext.get_websockets_raw(Some(tag));
        let mut result = Vec::new();
        for i in 0..js_array.length() {
            let ws_js = js_array.get(i);
            let web_sys_ws: worker::web_sys::WebSocket = ws_js.unchecked_into();
            let ws: WebSocket = web_sys_ws.into();
            result.push(ws);
        }
        result
    }

    pub fn send(&mut self, id: u32, message: String) -> bool {
        match self.connections.get_mut(&id) {
            Some(Connection::Active(ws)) => {
                let _ = ws.send_with_str(message);
                true
            }
            Some(Connection::Internal(_)) => {
                if let Some(Connection::Internal(sender)) = self.connections.remove(&id) {
                    let _ = sender.send(message);
                    true
                } else {
                    false
                }
            }
            None => false,
        }
    }

    pub fn len(&self) -> usize {
        self.state.get_websockets().len()
    }

    pub fn accept_web_socket(&self, ws: &WebSocket) {
        self.state.accept_web_socket(ws);
    }

    pub fn get_all_websockets(&self) -> Vec<WebSocket> {
        self.state.get_websockets()
    }

    pub fn remove(&mut self, id: u32) -> Option<Connection<WebSocket>> {
        self.connections.remove(&id)
    }
}
