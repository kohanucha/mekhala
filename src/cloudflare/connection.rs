use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use worker::*;
use crate::nostr::connection::ConnectionTransport;
use crate::util::short;

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

    pub fn identify(&self, ws: &WebSocket) -> Option<u32> {
        for (id, registered) in &self.websockets {
            if js_sys::Object::is(registered.as_ref(), ws.as_ref()) {
                crate::log_debug!("identify: conn={} found in active set", id);
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
            crate::log_debug!("find_by_id: conn={} found in active set", id);
            return Some(ws);
        }

        let ws_list: Vec<WebSocket> = state.get_websockets();
        crate::log_debug!("find_by_id: conn={} scanning {} hibernated websockets", id, ws_list.len());
        for ws in &ws_list {
            if let Ok(Some(attachment_id)) = ws.deserialize_attachment::<u32>() {
                if attachment_id == id {
                    crate::log_debug!("find_by_id: conn={} recovered from hibernation", id);
                    return Some(ws.clone());
                }
            }
        }
        crate::log_warn!("find_by_id: conn={} NOT FOUND ({} hibernated websockets scanned)", id, ws_list.len());
        None
    }

    pub fn send(&mut self, id: u32, message: String) -> bool {
        if let Some(ws) = self.websockets.get(&id) {
            let _ = ws.send_with_str(&message);
            crate::log_info!("→ conn={} SEND {}", id, short(&message, 120));
            true
        } else {
            crate::log_warn!("✗ send failed: conn={} not found", id);
            false
        }
    }

    pub fn active_count(&self) -> usize {
        self.websockets.len()
    }

    pub fn remove(&mut self, id: u32) -> Option<WebSocket> {
        self.websockets.remove(&id)
    }
}

/// Production adapter for ConnectionTransport.
/// Wraps WebSocketRegistry and provides Cloudflare-specific peer I/O.
pub struct CloudflareConnectionTransport {
    websockets: RefCell<WebSocketRegistry>,
    state: Rc<State>,
}

impl CloudflareConnectionTransport {
    pub fn new(state: Rc<State>) -> Self {
        Self {
            websockets: RefCell::new(WebSocketRegistry::new()),
            state,
        }
    }

}

impl ConnectionTransport for CloudflareConnectionTransport {
    type Connection = WebSocket;

    fn identify(&self, ws: &WebSocket) -> Option<u32> {
        self.websockets.borrow().identify(ws)
    }

    fn accept_and_register(&self, id: u32, ws: &WebSocket) {
        if let Err(e) = ws.serialize_attachment(&id) {
            crate::log_warn!("serialize_attachment failed for conn={}: {}", id, e);
        }
        self.state.accept_web_socket(ws);
        self.websockets.borrow_mut().add_active(id, ws.clone());
        crate::log_debug!("✓ conn={} registered, attachment verified", id);
    }

    fn send_to_peer(&self, id: u32, message: &str) -> bool {
        self.websockets.borrow_mut().send(id, message.to_string())
    }

    fn try_activate(&self, id: u32) -> bool {
        let ws = {
            let borrowed = self.websockets.borrow();
            borrowed.find_by_id(&self.state, id)
        };
        if let Some(ws) = ws {
            self.websockets.borrow_mut().add_active(id, ws);
            true
        } else {
            false
        }
    }

    fn remove_peer(&mut self, id: u32) {
        self.websockets.borrow_mut().remove(id);
    }

    fn active_count(&self) -> usize {
        self.websockets.borrow().active_count()
    }

    fn hibernated_count(&self) -> usize {
        self.state.get_websockets().len() - self.active_count()
    }
}
