use std::cell::RefCell;
use futures::channel::oneshot;
use futures::lock::Mutex;
use worker::*;
use crate::nostr::engine::{NostrEngine, EngineResponse, Storage};
use crate::cloudflare::create_cors_response;
use crate::cloudflare::connection::ConnectionRegistry;

pub struct CloudflareStorage {
    storage: worker::Storage,
}

#[async_trait::async_trait(?Send)]
impl Storage for CloudflareStorage {
    async fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.storage.get(key).await.ok().flatten()
    }
    async fn put_batch(&self, entries: std::collections::HashMap<String, serde_json::Value>) {
        for (k, v) in entries {
            let _ = self.storage.put(&k, v).await;
        }
    }
    async fn delete_batch(&self, keys: Vec<String>) {
        for k in keys {
            let _ = self.storage.delete(&k).await;
        }
    }
}

#[durable_object]
pub struct CloudflareTransport {
    env: Env,
    engine: Mutex<NostrEngine<CloudflareStorage>>,
    connections: RefCell<ConnectionRegistry>,
}

impl DurableObject for CloudflareTransport {
    fn new(state: State, env: Env) -> Self {
        let storage = CloudflareStorage { storage: state.storage() };
        let engine = NostrEngine::new_with_storage(storage);

        Self {
            env,
            engine: Mutex::new(engine),
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
            let mut engine = self.engine.lock().await;

            if text.len() > 65536 {
                let _ = websocket.send_with_str(&engine.error_message("message too large"));
                return Ok(());
            }
            let connection_id = self.wake_up_with_handler(&websocket, &mut engine).await.ok_or_else(|| Error::from("Connection not found"))?;

            let responses = engine.handle(connection_id, &text).await;
            self.process_responses(responses, &mut engine).await?;

            Ok(())
        } else {
            let engine = self.engine.lock().await;
            let _ = websocket.send_with_str(&engine.error_message("binary not supported"));
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

use futures_util::FutureExt;

#[async_trait::async_trait(?Send)]
impl crate::common::NwcTransport for CloudflareTransport {
    async fn get_wallet_info(&self, pubkey: &str) -> Option<crate::nostr::WalletInfo> {
        let mut engine = self.engine.lock().await;
        let _ = self.load_connection_with_handler(pubkey, &mut engine).await.ok();
        Some(engine.get_wallet_info(pubkey))
    }

    async fn execute_nwc_rpc(&self, request: crate::nostr::Event) -> Result<crate::nostr::Event> {
        let id = self.connections.borrow().next_id().await;
        let (tx, rx) = oneshot::channel();
        self.connections.borrow_mut().add_internal(id, tx);

        let mut machine = crate::nostr::rpc_machine::NwcRpcMachine::new(request);
        let mut engine = self.engine.lock().await;

        // 1. Execute initial actions (Subscribe, Publish)
        for action in machine.start() {
            self.execute_rpc_action(id, action, &mut engine).await?;
        }

        drop(engine); // Release lock while waiting

        // 2. Await response with timeout
        let start_time = crate::util::now();
        let timeout_sec = 10;
        let mut rx = rx;
        
        let result_event = loop {
            let elapsed = crate::util::now() - start_time;
            if elapsed >= timeout_sec {
                let action = machine.handle_timeout();
                let mut engine = self.engine.lock().await;
                let _ = self.execute_rpc_action(id, action, &mut engine).await;
                let _ = engine.on_disconnect(id).await;
                self.connections.borrow_mut().remove(id);
                return Err(Error::from("NWC RPC timeout"));
            }

            let delay = Delay::from(std::time::Duration::from_secs(timeout_sec - elapsed)).fuse();
            let (new_tx, new_rx) = oneshot::channel();
            
            let pinned_rx = rx;
            futures_util::pin_mut!(pinned_rx, delay);

            let response_text = match futures_util::future::select(pinned_rx, delay).await {
                futures_util::future::Either::Left((Ok(resp), _)) => resp,
                _ => {
                    let action = machine.handle_timeout();
                    let mut engine = self.engine.lock().await;
                    let _ = self.execute_rpc_action(id, action, &mut engine).await;
                    let _ = engine.on_disconnect(id).await;
                    self.connections.borrow_mut().remove(id);
                    return Err(Error::from("NWC RPC timeout"));
                }
            };

            // 3. Parse response and transition machine
            let msg = crate::nostr::RelayMessage::from_json(&response_text)
                .map_err(|e| Error::from(format!("malformed relay response: {}", e)))?;
            
            if let Some(action) = machine.transition(msg) {
                let mut engine = self.engine.lock().await;
                self.execute_rpc_action(id, action, &mut engine).await?;
            }

            match machine.state() {
                crate::nostr::rpc_machine::RpcState::Success(event) => break event.clone(),
                crate::nostr::rpc_machine::RpcState::Failed(err) => {
                    let mut engine = self.engine.lock().await;
                    let _ = engine.on_disconnect(id).await;
                    self.connections.borrow_mut().remove(id);
                    return Err(Error::from(err.clone()));
                }
                _ => {
                    // Continue waiting, setup next internal channel
                    self.connections.borrow_mut().add_internal(id, new_tx);
                    rx = new_rx;
                }
            }
        };

        // 4. Final Cleanup
        let mut engine = self.engine.lock().await;
        let _ = engine.on_disconnect(id).await;
        self.connections.borrow_mut().remove(id);

        Ok(result_event)
    }
}

impl CloudflareTransport {
    async fn execute_rpc_action(&self, conn_id: u32, action: crate::nostr::rpc_machine::RpcAction, engine: &mut NostrEngine<CloudflareStorage>) -> Result<()> {
        match action {
            crate::nostr::rpc_machine::RpcAction::Subscribe(sub_id, filter) => {
                let req_msg = crate::nostr::ClientMessage::Req(sub_id, vec![filter]);
                let responses = engine.handle_typed(conn_id, req_msg).await;
                self.process_responses(responses, engine).await?;
            }
            crate::nostr::rpc_machine::RpcAction::Publish(event) => {
                let event_msg = crate::nostr::ClientMessage::Event(event);
                let responses = engine.handle_typed(conn_id, event_msg).await;
                self.process_responses(responses, engine).await?;
            }
            crate::nostr::rpc_machine::RpcAction::Unsubscribe(sub_id) => {
                let responses = engine.process_close(conn_id, sub_id).await;
                self.process_responses(responses, engine).await?;
            }
        }
        Ok(())
    }

    async fn load_connection_with_handler(&self, pubkey: &str, engine: &mut NostrEngine<CloudflareStorage>) -> Result<Option<u32>> {
        if let Some(id) = engine.load_by_pubkey(pubkey).await {
            let ws = self.connections.borrow().find_by_id(id);
            if let Some(ws) = ws {
                let _ = self.wake_up_with_handler(&ws, engine).await;
                return Ok(Some(id));
            }

            // Fallback: search all sockets if tagged one failed to wake up
            let all_ws = self.connections.borrow().get_all_websockets();
            for ws in all_ws {
                if let Some(actual_id) = self.wake_up_with_handler(&ws, engine).await {
                    if actual_id == id {
                        return Ok(Some(id));
                    }
                }
            }
            return Ok(Some(id));
        }
        Ok(None)
    }

    async fn wake_up_with_handler(&self, ws: &WebSocket, engine: &mut NostrEngine<CloudflareStorage>) -> Option<u32> {
        let id = self.connections.borrow().identify(ws)?;

        // Attempt to load state, but don't fail if there is no state yet (e.g. new connection)
        let _ = engine.load(id).await;
        
        // Ensure it's in memory as an active connection
        self.connections.borrow_mut().add_active(id, ws.clone());
        Some(id)
    }

    async fn accept_new_connection(&self) -> Result<Response> {
        let max_connections = self.env.var("MAX_CONNECTIONS")
            .and_then(|v| v.to_string().parse::<usize>().map_err(|e| Error::from(e.to_string())))
            .unwrap_or(20);

        if self.connections.borrow().len() >= max_connections {
            return Response::error("Too Many Requests", 429);
        }

        let WebSocketPair { client, server } = WebSocketPair::new()?;
        
        {
            let connections = self.connections.borrow();
            connections.accept_web_socket(&server);
        }

        let mut engine = self.engine.lock().await;
        let connection_id = self.connections.borrow().next_id().await;
        let responses = engine.on_connect(connection_id).await;
        
        self.connections.borrow_mut().register_active(connection_id, server);

        self.process_responses(responses, &mut engine).await?;

        Ok(Response::from_websocket(client)?)
    }

    async fn handle_disconnect(&self, ws: &WebSocket) -> Result<()> {
        let mut engine = self.engine.lock().await;
        if let Some(id) = self.wake_up_with_handler(ws, &mut engine).await {
            let responses = engine.on_terminate(id).await;
            self.process_responses(responses, &mut engine).await?;
            self.connections.borrow_mut().remove(id);
        }
        Ok(())
    }

    async fn process_responses(&self, responses: Vec<EngineResponse>, engine: &mut NostrEngine<CloudflareStorage>) -> Result<()> {
        for resp in responses {
            match resp {
                EngineResponse::Data { recipient_id, message } => {
                    self.connections.borrow_mut().send(recipient_id, message.to_json());
                }
                EngineResponse::Reply { recipient_id, message } => {
                    // Delivery Policy: Suppress protocol acknowledgments for internal/bridge connections
                    if !self.connections.borrow().is_internal(recipient_id) {
                        self.connections.borrow_mut().send(recipient_id, message.to_json());
                    }
                }
                EngineResponse::WakeUp { connection_id } => {
                    let ws = self.connections.borrow().find_by_id(connection_id);
                    if let Some(ws) = ws {
                        let _ = self.wake_up_with_handler(&ws, engine).await;
                    }
                }
            }
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
