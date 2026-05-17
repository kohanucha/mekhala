use std::collections::HashMap;
use std::cell::Cell;
use futures::channel::oneshot;
use worker::*;
use wasm_bindgen::JsCast;
use crate::cloudflare::HibernationState;

pub struct WebSocketRegistry {
    state: State,
    websockets: HashMap<u32, WebSocket>,
    current_id: Cell<u32>,
    id_limit: Cell<u32>,
}

impl WebSocketRegistry {
    pub fn new(state: State) -> Self {
        Self {
            state,
            websockets: HashMap::new(),
            current_id: Cell::new(0),
            id_limit: Cell::new(0),
        }
    }

    pub fn add_active(&mut self, id: u32, ws: WebSocket) {
        self.websockets.insert(id, ws);
    }

    pub fn register_active(&mut self, id: u32, ws: WebSocket) {
        let tag = format!("id:{}", id);
        self.state.set_tags(&ws, vec![tag]);
        self.add_active(id, ws);
    }

    pub fn identify(&self, ws: &WebSocket) -> Option<u32> {
        for (id, registered) in &self.websockets {
            if js_sys::Object::is(registered.as_ref(), ws.as_ref()) {
                return Some(*id);
            }
        }

        let tags = self.state.get_tags(ws);
        let id_tag = tags.iter().find(|t| t.starts_with("id:"))?;
        id_tag.strip_prefix("id:")?.parse().ok()
    }

    pub async fn next_id(&self) -> u32 {
        let current = self.current_id.get();
        let limit = self.id_limit.get();

        if current >= limit {
            let storage = self.state.storage();
            let last_limit = storage.get::<u32>("id_counter").await.ok().flatten().unwrap_or(0);

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
        self.websockets.get(&id).cloned()
    }

    pub fn find_by_id(&self, id: u32) -> Option<WebSocket> {
        if let Some(ws) = self.get_active(id) {
            return Some(ws);
        }

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
        if let Some(ws) = self.websockets.get(&id) {
            let _ = ws.send_with_str(&message);
            true
        } else {
            false
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

    pub fn remove(&mut self, id: u32) -> Option<WebSocket> {
        self.websockets.remove(&id)
    }
}

pub struct InternalConnectionMap {
    channels: HashMap<u32, oneshot::Sender<String>>,
}

impl InternalConnectionMap {
    pub fn new() -> Self {
        Self {
            channels: HashMap::new(),
        }
    }

    pub fn add(&mut self, id: u32, sender: oneshot::Sender<String>) {
        self.channels.insert(id, sender);
    }

    pub fn send(&mut self, id: u32, message: String) -> bool {
        if let Some(sender) = self.channels.remove(&id) {
            let _ = sender.send(message);
            true
        } else {
            false
        }
    }

    pub fn remove(&mut self, id: u32) {
        self.channels.remove(&id);
    }
}