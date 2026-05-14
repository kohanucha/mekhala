use std::cell::RefCell;
use futures::channel::oneshot;
use futures::lock::Mutex;
use worker::*;
use crate::nostr::engine::{NostrEngine, EngineResponse, MessageFlags};
use crate::nostr::protocol_handler::NostrProtocolHandler;
use crate::nostr::wallet_registry::{WalletRegistry, Storage};
use crate::cloudflare::create_cors_response;
use crate::cloudflare::connection::ConnectionRegistry;

pub struct CloudflareStorage {
    storage: worker::Storage,
}

#[async_trait::async_trait(?Send)]
impl Storage for CloudflareStorage {
    async fn put(&self, key: &str, value: serde_json::Value) {
        let _ = self.storage.put(key, value).await;
    }
    async fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.storage.get(key).await.ok().flatten()
    }
    async fn delete(&self, key: &str) {
        let _ = self.storage.delete(key).await;
    }
}

#[durable_object]
pub struct CloudflareTransport {
    env: Env,
    handler: Mutex<NostrProtocolHandler<CloudflareStorage>>,
    connections: RefCell<ConnectionRegistry>,
}

impl DurableObject for CloudflareTransport {
    fn new(state: State, env: Env) -> Self {
        let storage = CloudflareStorage { storage: state.storage() };
        let registry = WalletRegistry::new(storage);
        let engine = NostrEngine { registry };
        let handler = NostrProtocolHandler::new(engine);

        Self {
            env,
            handler: Mutex::new(handler),
            connections: RefCell::new(ConnectionRegistry::new(state)),
        }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path();

        // Handle delegated LN Address callback
        if path.starts_with("/lnaddress/") && path.ends_with("/callback") {
            let username = path.strip_prefix("/lnaddress/").and_then(|s| s.strip_suffix("/callback")).unwrap_or("");
            return crate::lnaddress::handle_lnaddress_callback(req, &self.env, username, self).await;
        }

        self.accept_new_connection().await
    }

    async fn websocket_message(&self, websocket: WebSocket, message: WebSocketIncomingMessage) -> Result<()> {
        if let WebSocketIncomingMessage::String(text) = message {
            let mut handler = self.handler.lock().await;

            if text.len() > 65536 {
                let _ = websocket.send_with_str(&handler.engine.error_message("message too large"));
                return Ok(());
            }
            let connection_id = self.wake_up_with_handler(&websocket, &mut handler).await.ok_or_else(|| Error::from("Connection not found"))?;

            let responses = handler.handle(connection_id, &text, MessageFlags::default()).await;
            self.process_responses(responses, &mut handler).await?;

            Ok(())
        } else {
            let handler = self.handler.lock().await;
            let _ = websocket.send_with_str(&handler.engine.error_message("binary not supported"));
            Ok(())
        }
    }

    async fn websocket_close(&self, ws: WebSocket, _: usize, _: String, _: bool) -> Result<()> {
        self.handle_disconnect(&ws).await
    }

    async fn websocket_error(&self, ws: WebSocket, _: Error) -> Result<()> {
        self.handle_disconnect(&ws).await
    }
}

#[async_trait::async_trait(?Send)]
impl crate::common::InternalTransport for CloudflareTransport {
    async fn get_wallet_info(&self, pubkey: &str) -> Option<crate::nostr::WalletInfo> {
        let mut handler = self.handler.lock().await;
        let _ = self.load_connection_with_handler(pubkey, &mut handler).await.ok();
        Some(handler.engine.get_wallet_info(pubkey))
    }

    async fn send_message(&self, id: u32, message: String, sender: oneshot::Sender<String>) -> Result<()> {
        self.connections.borrow_mut().add_internal(id, sender);

        let mut handler = self.handler.lock().await;

        let responses = handler.handle(id, &message, MessageFlags { is_internal: true }).await;
        self.process_responses(responses, &mut handler).await?;
        Ok(())
    }

    async fn generate_id(&self) -> u32 {
        self.connections.borrow().next_id().await
    }

    async fn close_connection(&self, id: u32) -> Result<()> {
        let mut handler = self.handler.lock().await;
        let responses = handler.engine.on_disconnect(id).await;
        self.process_responses(responses, &mut handler).await?;
        self.connections.borrow_mut().remove(id);
        Ok(())
    }
}

impl CloudflareTransport {
    async fn load_connection_with_handler(&self, pubkey: &str, handler: &mut NostrProtocolHandler<CloudflareStorage>) -> Result<Option<u32>> {
        if let Some(id) = handler.engine.registry.load_by_pubkey(pubkey).await {
            let ws = self.connections.borrow().find_by_id(id);
            if let Some(ws) = ws {
                let _ = self.wake_up_with_handler(&ws, handler).await;
            } else {
                // Fallback: search all sockets if tagged one failed to wake up
                let all_ws = self.connections.borrow().get_all_websockets();
                for ws in all_ws {
                    if let Some(actual_id) = self.wake_up_with_handler(&ws, handler).await {
                        if actual_id == id {
                            return Ok(Some(id));
                        }
                    }
                }
            }
            return Ok(Some(id));
        }
        Ok(None)
    }

    async fn process_responses(&self, responses: Vec<EngineResponse>, handler: &mut NostrProtocolHandler<CloudflareStorage>) -> Result<()> {
        for resp in responses {
            match resp {
                EngineResponse::Send { connection_id, message } => {
                    self.connections.borrow_mut().send(connection_id, message);
                }
                EngineResponse::WakeUp { connection_id } => {
                    let ws = self.connections.borrow().find_by_id(connection_id);
                    if let Some(ws) = ws {
                        let _ = self.wake_up_with_handler(&ws, handler).await;
                    }
                }
            }
        }
        Ok(())
    }

    async fn wake_up_with_handler(&self, ws: &WebSocket, handler: &mut NostrProtocolHandler<CloudflareStorage>) -> Option<u32> {
        let id = self.connections.borrow().identify(ws)?;

        // Attempt to load state, but don't fail if there is no state yet (e.g. new connection)
        let _ = handler.engine.registry.load(id).await;
        
        // Ensure it's in memory as an active connection
        self.connections.borrow_mut().add_active(id, ws.clone());
        Some(id)
    }

    async fn accept_new_connection(&self) -> Result<Response> {
        let WebSocketPair { client, server } = WebSocketPair::new()?;
        
        {
            let connections = self.connections.borrow();
            connections.accept_web_socket(&server);
        }

        let mut handler = self.handler.lock().await;
        let connection_id = self.connections.borrow().next_id().await;
        let responses = handler.engine.on_connect(connection_id).await;
        
        self.connections.borrow_mut().register_active(connection_id, server);

        self.process_responses(responses, &mut handler).await?;

        Ok(Response::from_websocket(client)?)
    }

    async fn handle_disconnect(&self, ws: &WebSocket) -> Result<()> {
        let mut handler = self.handler.lock().await;
        if let Some(id) = self.wake_up_with_handler(ws, &mut handler).await {
            let responses = handler.engine.on_disconnect(id).await;
            self.process_responses(responses, &mut handler).await?;
            
            let storage = {
                let registry = self.connections.borrow();
                registry.storage()
            };
            
            self.connections.borrow_mut().remove(id);
            
            // Clean up storage
            let _ = storage.delete(&format!("conn:{}", id)).await;
        }
        Ok(())
    }
}

pub async fn connect(req: Request, env: &Env) -> Result<Response> {
    crate::cloudflare::get_durable_stub(env)?.fetch_with_request(req).await
}

pub fn create_response(info: serde_json::Value, content_type: &str) -> Result<Response> {
    let mut response = create_cors_response(Response::from_json(&info)?)?;
    response.headers_mut().set("Content-Type", content_type)?;
    Ok(response)
}
