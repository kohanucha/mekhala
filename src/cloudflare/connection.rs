use std::collections::HashMap;
use futures::channel::oneshot;
use worker::*;

pub struct WebSocketRegistry {
    websockets: HashMap<u32, WebSocket>,
}

impl WebSocketRegistry {
    pub fn new() -> Self {
        Self {
            websockets: HashMap::new(),
        }
    }

    pub fn add_active(&mut self, id: u32, ws: WebSocket) {
        self.websockets.insert(id, ws);
    }

    pub fn accept_and_register(&mut self, state: &State, id: u32, ws: &WebSocket) {
        state.accept_web_socket(ws);
        self.add_active(id, ws.clone());
        if let Err(e) = ws.serialize_attachment(&id) {
            crate::log_warn!("serialize_attachment failed for conn={}: {}", id, e);
        }
        crate::log_debug!("✓ conn={} registered, attachment verified", id);
    }

    pub fn identify(&self, ws: &WebSocket) -> Option<u32> {
        for (id, registered) in &self.websockets {
            if js_sys::Object::is(registered.as_ref(), ws.as_ref()) {
                return Some(*id);
            }
        }

        let id: u32 = ws.deserialize_attachment::<u32>().ok().flatten()?;
        crate::log_debug!("identify: recovered conn={} from hibernation attachment", id);
        Some(id)
    }

    pub fn get_active(&self, id: u32) -> Option<WebSocket> {
        self.websockets.get(&id).cloned()
    }

    pub fn find_by_id(&self, state: &State, id: u32) -> Option<WebSocket> {
        if let Some(ws) = self.get_active(id) {
            return Some(ws);
        }

        for ws in state.get_websockets() {
            if let Ok(Some(attachment_id)) = ws.deserialize_attachment::<u32>() {
                if attachment_id == id {
                    return Some(ws);
                }
            }
        }
        None
    }

    pub fn send(&mut self, id: u32, message: String) -> bool {
        if let Some(ws) = self.websockets.get(&id) {
            let _ = ws.send_with_str(&message);
            true
        } else {
            false
        }
    }

    pub fn len(&self, state: &State) -> usize {
        state.get_websockets().len()
    }

    pub fn get_all_websockets(&self, state: &State) -> Vec<WebSocket> {
        state.get_websockets()
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
