use super::{Event, RelayMessage, ClientMessage, MessageFlags};
use super::engine::{NostrEngine, EngineResponse};
use super::wallet_registry::Storage;
use crate::util::now;

pub struct NostrProtocolHandler<S: Storage> {
    pub engine: NostrEngine<S>,
}

impl<S: Storage> NostrProtocolHandler<S> {
    pub fn new(engine: NostrEngine<S>) -> Self {
        Self { engine }
    }

    pub async fn handle(&mut self, connection_id: u32, message: &str, flags: MessageFlags) -> Vec<EngineResponse> {
        match ClientMessage::from_json(message) {
            Ok(ClientMessage::Event(event)) => self.handle_event(connection_id, event, flags).await,
            Ok(ClientMessage::Req(sub_id, filters)) => self.handle_req(connection_id, sub_id, filters, flags).await,
            Ok(ClientMessage::Close(sub_id)) => self.engine.process_close(connection_id, sub_id).await,
            Err(e) => {
                if !flags.is_internal {
                    vec![EngineResponse::send(connection_id, RelayMessage::Notice(format!("parse failed: {}", e)).to_json())]
                } else {
                    Vec::new()
                }
            }
        }
    }

    async fn handle_event(&mut self, connection_id: u32, event: Event, flags: MessageFlags) -> Vec<EngineResponse> {
        // 1. Kind validation
        match event.kind {
            13194 | 23194..=23197 => {
                // 2. Protocol verification
                if let Err(e) = event.verify(now()) {
                    if !flags.is_internal {
                        return vec![EngineResponse::send(connection_id, RelayMessage::Ok(event.id, false, e.to_string()).to_json())];
                    } else {
                        return Vec::new();
                    }
                }
                
                // 3. Dispatch to engine
                if event.kind == 13194 {
                    self.engine.process_info_event(event).await;
                    Vec::new()
                } else {
                    self.engine.process_event(connection_id, event, flags).await
                }
            }
            _ => {
                if !flags.is_internal {
                    let message = if let Err(e) = event.verify(now()) {
                        RelayMessage::Ok(event.id, false, e.to_string())
                    } else {
                        RelayMessage::Ok(event.id, false, "blocked: event kind not allowed".into())
                    };
                    
                    vec![EngineResponse::send(connection_id, message.to_json())]
                } else {
                    Vec::new()
                }
            }
        }
    }

    async fn handle_req(&mut self, id: u32, sub_id: String, filters: Vec<crate::nostr::Filter>, flags: MessageFlags) -> Vec<EngineResponse> {
        if filters.iter().any(|f| !f.is_valid()) {
            let message = RelayMessage::Closed(sub_id.clone(), "filter too broad".to_string()).to_json();
            
            if !flags.is_internal {
                return vec![EngineResponse::send(id, message)];
            } else {
                return Vec::new();
            }
        }

        self.engine.process_req(id, sub_id, filters, flags).await
    }
}
