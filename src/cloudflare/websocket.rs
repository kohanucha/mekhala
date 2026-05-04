use std::cell::RefCell;
use worker::*;
use crate::cloudflare::{accept_connection, HibernationState};
use crate::util::now;
use crate::nostr::{RelayMessage, engine::{NostrEngine, NostrTransport}, state::ConnectionState};
use crate::cloudflare::apply_security_headers;

#[durable_object]
pub struct Websocket {
    state: State,
    engine: RefCell<NostrEngine>,
    id_map: RefCell<Vec<(WebSocket, u32)>>,
}

impl DurableObject for Websocket {
    fn new(state: State, _env: Env) -> Self {
        let mut engine = NostrEngine::new();
        let mut id_map = Vec::new();

        for ws in state.get_websockets() {
            if let Ok(Some(conn_state)) = ws.deserialize_attachment::<ConnectionState>() {
                engine.add_connection(conn_state.id, conn_state.clone());
                id_map.push((ws, conn_state.id));
            }
        }

        Self {
            state,
            engine: RefCell::new(engine),
            id_map: RefCell::new(id_map),
        }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path();

        if path.starts_with("/check/") {
            let pubkey = path.strip_prefix("/check/").unwrap_or("");
            let info = self.engine.borrow().get_wallet_info(pubkey);
            let is_online = info.get("online").and_then(|v| v.as_bool()).unwrap_or(false);
            return apply_security_headers(Response::ok(if is_online { "OK" } else { "OFFLINE" })?);
        }

        if path.starts_with("/info/") {
            let pubkey = path.strip_prefix("/info/").unwrap_or("");
            let info = self.engine.borrow().get_wallet_info(pubkey);
            return apply_security_headers(Response::from_json(&info)?);
        }

        self.accept_new_connection()
    }

    async fn websocket_message(&self, ws: WebSocket, message: WebSocketIncomingMessage) -> Result<()> {
        if let WebSocketIncomingMessage::String(text) = message {
            if text.len() > 65536 {
                ws.send_with_str(&RelayMessage::Notice("message too large".to_string()).to_json())?;
                return Ok(());
            }

            let arr: Vec<serde_json::Value> = match serde_json::from_str(&text) {
                Ok(a) => a,
                Err(e) => {
                    ws.send_with_str(&RelayMessage::Notice(format!("parse failed: {}", e)).to_json())?;
                    return Ok(());
                }
            };

            if arr.is_empty() {
                return Ok(());
            }

            match arr[0].as_str() {
                Some("EVENT") if arr.len() >= 2 => self.handle_event(&ws, &arr),
                Some("REQ") if arr.len() >= 3 => self.handle_req(&ws, &arr),
                Some("CLOSE") if arr.len() >= 2 => self.handle_close(&ws, &arr[1]),
                _ => Ok(()),
            }
        } else {
            ws.send_with_str(&RelayMessage::Notice("binary not supported".to_string()).to_json())?;
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

impl NostrTransport for Websocket {
    fn send(&self, id: u32, message: RelayMessage) {
        let id_map = self.id_map.borrow();
        if let Some((ws, _)) = id_map.iter().find(|(_, i)| *i == id) {
            let _ = ws.send_with_str(&message.to_json());
        }
    }

    fn persist(&self, id: u32, state: &ConnectionState) {
        let id_map = self.id_map.borrow();
        if let Some((ws, _)) = id_map.iter().find(|(_, i)| *i == id) {
            let _ = ws.serialize_attachment(state);
        }
    }

    fn set_tags(&self, id: u32, tags: Vec<String>) {
        let id_map = self.id_map.borrow();
        if let Some((ws, _)) = id_map.iter().find(|(_, i)| *i == id) {
            self.state.set_tags(ws, tags);
        }
    }
}

impl Websocket {
    fn get_id(&self, ws: &WebSocket) -> Option<u32> {
        self.id_map.borrow().iter().find(|(w, _)| ws_eq(w, ws)).map(|(_, id)| *id)
    }

    fn accept_new_connection(&self) -> Result<Response> {
        let resp = accept_connection(&self.state, 100)?;
        
        let active_ws = self.state.get_websockets();
        let mut id_map = self.id_map.borrow_mut();
        
        if let Some(new_ws) = active_ws.into_iter().find(|aw| !id_map.iter().any(|(w, _)| ws_eq(w, aw))) {
            let max_id = id_map.iter().map(|(_, id)| *id).max().unwrap_or(0);
            let new_id = max_id + 1;
            
            let mut conn_state = ConnectionState::default();
            conn_state.id = new_id;
            
            new_ws.serialize_attachment(&conn_state)?;
            self.engine.borrow_mut().add_connection(new_id, conn_state);
            id_map.push((new_ws, new_id));
        }

        Ok(resp)
    }

    fn handle_event(&self, ws: &WebSocket, arr: &[serde_json::Value]) -> Result<()> {
        let event: crate::nostr::Event = serde_json::from_value(arr[1].clone())
            .map_err(|e| Error::from(e.to_string()))?;

        let now = now();

        if let Err(e) = event.verify(now) {
            ws.send_with_str(&RelayMessage::Ok(event.id, false, e.to_string()).to_json())?;
            return Ok(());
        }

        ws.send_with_str(&RelayMessage::Ok(event.id.clone(), true, "".into()).to_json())?;

        let id = self.get_id(ws).ok_or_else(|| Error::from("Connection not found"))?;

        let mut engine = self.engine.borrow_mut();
        if event.kind == 13194 {
            engine.save_info_event(self, id, event.clone());
        }

        engine.handle_event(self, &event);

        Ok(())
    }

    fn handle_req(&self, ws: &WebSocket, arr: &[serde_json::Value]) -> Result<()> {
        let sub_id = arr[1].as_str().unwrap_or("");
        
        let mut filters = Vec::new();
        for value in &arr[2..] {
            let filter: crate::nostr::Filter = serde_json::from_value(value.clone())
                .map_err(|e| Error::from(e.to_string()))?;
            filters.push(filter);
        }

        if filters.iter().any(|f| !f.is_valid()) {
            ws.send_with_str(&RelayMessage::Closed(sub_id.to_string(), "filter too broad".to_string()).to_json())?;
            return Ok(());
        }

        let id = self.get_id(ws).ok_or_else(|| Error::from("Connection not found"))?;
        
        self.engine.borrow_mut().subscribe(self, id, sub_id.to_string(), filters);

        ws.send_with_str(&RelayMessage::Eose(sub_id.to_string()).to_json())?;
        
        Ok(())
    }

    fn handle_close(&self, ws: &WebSocket, sub_id: &serde_json::Value) -> Result<()> {
        let sub_id = sub_id.as_str().unwrap_or("");
        let id = self.get_id(ws).ok_or_else(|| Error::from("Connection not found"))?;
        self.engine.borrow_mut().unsubscribe(self, id, Some(sub_id.to_string()));
        Ok(())
    }

    fn handle_disconnect(&self, ws: &WebSocket) -> Result<()> {
        if let Some(id) = self.get_id(ws) {
            self.engine.borrow_mut().remove_connection(id);
            self.id_map.borrow_mut().retain(|(_, i)| *i != id);
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
