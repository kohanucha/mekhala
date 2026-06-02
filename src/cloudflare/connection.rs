use std::collections::HashMap;
use futures::channel::oneshot;
use worker::*;

pub enum ConnectionKind {
    External(WebSocket),
    Internal(oneshot::Sender<String>),
}

pub struct ConnectionRegistry {
    connections: HashMap<u32, ConnectionKind>,
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
        }
    }

    pub fn accept_and_register(&mut self, state: &State, id: u32, ws: &WebSocket) {
        if let Err(e) = ws.serialize_attachment(id) {
            crate::log_warn!("serialize_attachment failed for conn={}: {}", id, e);
        }
        state.accept_web_socket(ws);
        self.connections.insert(id, ConnectionKind::External(ws.clone()));
        crate::log_debug!("✓ conn={} registered, attachment verified", id);
    }

    pub fn insert_active(&mut self, id: u32, ws: WebSocket) {
        self.connections.insert(id, ConnectionKind::External(ws));
    }

    pub fn add_internal(&mut self, id: u32, sender: oneshot::Sender<String>) {
        self.connections.insert(id, ConnectionKind::Internal(sender));
    }

    pub fn identify(&self, ws: &WebSocket) -> Option<u32> {
        for (id, kind) in &self.connections {
            if let ConnectionKind::External(registered) = kind {
                if js_sys::Object::is(registered.as_ref(), ws.as_ref()) {
                    return Some(*id);
                }
            }
        }

        let id: u32 = ws.deserialize_attachment::<u32>().ok().flatten()?;
        crate::log_debug!("identify: recovered conn={} from hibernation attachment", id);
        Some(id)
    }

    pub fn find_ws_by_id(&self, state: &State, id: u32) -> Option<WebSocket> {
        for ws in state.get_websockets() {
            if let Ok(Some(attachment_id)) = ws.deserialize_attachment::<u32>() {
                if attachment_id == id {
                    return Some(ws);
                }
            }
        }

        if let Some(ConnectionKind::External(ws)) = self.connections.get(&id) {
            return Some(ws.clone());
        }

        None
    }

    pub fn get_all_websockets(&self, state: &State) -> Vec<WebSocket> {
        state.get_websockets()
    }

    pub fn len(&self, state: &State) -> usize {
        state.get_websockets().len()
    }

    pub fn send(&mut self, id: u32, message: String) -> bool {
        match self.connections.get(&id) {
            Some(ConnectionKind::External(ws)) => {
                let _ = ws.send_with_str(&message);
                true
            }
            Some(ConnectionKind::Internal(_)) => {
                if let Some(ConnectionKind::Internal(sender)) = self.connections.remove(&id) {
                    sender.send(message).is_ok()
                } else {
                    false
                }
            }
            None => false,
        }
    }

    pub fn remove(&mut self, id: u32) {
        self.connections.remove(&id);
    }
}

#[cfg(test)]
#[path = "connection_test.rs"]
mod connection_test;
