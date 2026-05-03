use worker::*;
use crate::model::Limits;
use crate::cloudflare::{accept_connection, SubscriptionManager};
use crate::util::now;
use crate::nostr::RelayMessage;
use crate::cloudflare::apply_security_headers;

#[durable_object]
pub struct Websocket {
    state: State,
    manager: SubscriptionManager,
}

impl DurableObject for Websocket {
    fn new(state: State, _env: Env) -> Self {
        let manager = SubscriptionManager::new(&state);
        Self {
            state,
            manager,
        }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path();

        if path.starts_with("/check/") {
            let pubkey = path.strip_prefix("/check/").unwrap_or("");
            let is_online = self.manager.is_wallet_online(pubkey);
            return apply_security_headers(Response::ok(if is_online { "OK" } else { "OFFLINE" })?);
        }

        accept_connection(&self.state, 100)
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
                    ws.send_with_str(&RelayMessage::Notice(format!("parse error: {}", e)).to_json())?;
                    return Ok(());
                }
            };

            if arr.is_empty() {
                return Ok(());
            }

            match arr[0].as_str() {
                Some("EVENT") if arr.len() >= 3 => self.handle_event(&ws, &arr),
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

impl Websocket {
    fn handle_event(&self, ws: &WebSocket, arr: &[serde_json::Value]) -> Result<()> {
        let event: crate::model::Event = serde_json::from_value(arr[2].clone())
            .map_err(|e| Error::from(e.to_string()))?;

        let now = now();
        let limits = Limits::default();

        if let Err(e) = event.verify(now, &limits) {
            ws.send_with_str(&RelayMessage::Ok(event.id, false, e.to_string()).to_json())?;
            return Ok(());
        }

        ws.send_with_str(&RelayMessage::Ok(event.id.clone(), true, "".into()).to_json())?;

        self.manager.broadcast(&event)?;

        Ok(())
    }

    fn handle_req(&self, ws: &WebSocket, arr: &[serde_json::Value]) -> Result<()> {
        let sub_id = arr[1].as_str().unwrap_or("");
        let filters: Vec<crate::model::Filter> = serde_json::from_value(arr[2].clone())
            .map_err(|e| Error::from(e.to_string()))?;

        if filters.iter().any(|f| !f.is_valid(&Limits::default())) {
            ws.send_with_str(&RelayMessage::Closed(sub_id.to_string(), "filter too broad".to_string()).to_json())?;
            return Ok(());
        }

        self.manager.subscribe(&self.state, ws, sub_id.to_string(), filters)?;

        ws.send_with_str(&RelayMessage::Eose(sub_id.to_string()).to_json())?;

        Ok(())
    }

    fn handle_close(&self, ws: &WebSocket, sub_id: &serde_json::Value) -> Result<()> {
        let sub_id = sub_id.as_str().unwrap_or("");
        self.manager.unsubscribe(&self.state, ws, Some(sub_id.to_string()))
    }

    fn handle_disconnect(&self, ws: &WebSocket) -> Result<()> {
        self.manager.unsubscribe(&self.state, ws, None)
    }
}