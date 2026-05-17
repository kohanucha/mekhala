use std::cell::RefCell;
use std::time::Duration;
use futures::channel::oneshot;
use futures::lock::Mutex;
use futures_util::FutureExt;
use worker::*;
use crate::nostr::engine::{NostrEngine, EngineResponse};
use crate::nostr::wallet_registry::Storage;
use crate::nostr::Limits;
use crate::nostr::rpc_machine::RpcAction;
use crate::nostr::rpc_orchestrator::{RpcContext, RpcReceiveError};
use crate::cloudflare::create_cors_response;
use crate::cloudflare::connection::{WebSocketRegistry, InternalConnectionMap};
use crate::cloudflare::kv::CloudflareKvStore;

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
    websockets: RefCell<WebSocketRegistry>,
    internal: RefCell<InternalConnectionMap>,
    kv: CloudflareKvStore,
}

impl DurableObject for CloudflareTransport {
    fn new(state: State, env: Env) -> Self {
        let storage = CloudflareStorage { storage: state.storage() };
        let limits = Limits::new(
            env.var("MAX_FILTER_ITEMS")
                .and_then(|v| v.to_string().parse::<usize>().map_err(|e| Error::from(e.to_string())))
                .unwrap_or(10),
            env.var("MAX_EVENT_TAGS")
                .and_then(|v| v.to_string().parse::<usize>().map_err(|e| Error::from(e.to_string())))
                .unwrap_or(10),
            env.var("MAX_CONTENT_LENGTH")
                .and_then(|v| v.to_string().parse::<usize>().map_err(|e| Error::from(e.to_string())))
                .unwrap_or(16384),
        );
        let engine = NostrEngine::new_with_storage(storage, limits, crate::util::now);
        let kv = CloudflareKvStore::new(env.kv("MEKHALA_NWC_KV").expect("MEKHALA_NWC_KV not configured"));

        Self {
            env,
            engine: Mutex::new(engine),
            websockets: RefCell::new(WebSocketRegistry::new(state)),
            internal: RefCell::new(InternalConnectionMap::new()),
            kv,
        }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path();

        if path.starts_with("/lnaddress/") && path.ends_with("/callback") {
            let username = path.strip_prefix("/lnaddress/").and_then(|s| s.strip_suffix("/callback")).unwrap_or("");
            let handler = crate::lnaddress::LnAddressHandler::new(&self.kv);
            return handler.handle_callback(req, username, self).await;
        }

        self.accept_new_connection().await
    }

    async fn websocket_message(&self, websocket: WebSocket, message: WebSocketIncomingMessage) -> Result<()> {
        if let WebSocketIncomingMessage::String(text) = message {
            let mut engine = self.engine.lock().await;

            if text.len() > 65536 {
                let _ = websocket.send_with_str(&crate::nostr::RelayMessage::Notice("message too large".into()).to_json());
                return Ok(());
            }
            let connection_id = self.wake_up_with_handler(&websocket, &mut engine).await.ok_or_else(|| Error::from("Connection not found"))?;

            let responses = engine.handle(connection_id, &text).await;
            self.process_responses(responses, &mut engine).await?;

            Ok(())
        } else {
            let _engine = self.engine.lock().await;
            let _ = websocket.send_with_str(&crate::nostr::RelayMessage::Notice("binary not supported".into()).to_json());
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
impl crate::common::NwcTransport for CloudflareTransport {
    async fn get_wallet_info(&self, pubkey: &str) -> Option<crate::nostr::WalletInfo> {
        let mut engine = self.engine.lock().await;
        let _ = self.load_connection_with_handler(pubkey, &mut engine).await.ok();
        engine.get_wallet_info(pubkey)
    }

    async fn execute_nwc_rpc(&self, request: crate::nostr::Event) -> Result<crate::nostr::Event, crate::common::NwcError> {
        crate::nostr::rpc_orchestrator::execute_nwc_rpc(self, request).await
    }
}

#[async_trait::async_trait(?Send)]
impl RpcContext for CloudflareTransport {
    fn now(&self) -> u64 {
        crate::util::now()
    }

    async fn allocate_connection_id(&self) -> u32 {
        self.websockets.borrow().next_id().await
    }

    async fn execute_action(&self, conn_id: u32, action: RpcAction) -> Result<(), crate::common::NwcError> {
        let mut engine = self.engine.lock().await;
        self.execute_rpc_action_inner(conn_id, action, &mut engine).await
    }

    async fn receive_response(&self, conn_id: u32, remaining_secs: u64) -> Result<String, RpcReceiveError> {
        let (tx, rx) = oneshot::channel();
        self.internal.borrow_mut().add(conn_id, tx);

        let delay = Delay::from(Duration::from_secs(remaining_secs)).fuse();
        let pinned_rx = rx.fuse();
        futures_util::pin_mut!(pinned_rx, delay);

        match futures_util::future::select(pinned_rx, delay).await {
            futures_util::future::Either::Left((Ok(resp), _)) => Ok(resp),
            futures_util::future::Either::Left((Err(_), _)) => Err(RpcReceiveError::ChannelClosed),
            futures_util::future::Either::Right(_) => {
                self.internal.borrow_mut().remove(conn_id);
                Err(RpcReceiveError::Timeout)
            }
        }
    }

    async fn disconnect(&self, conn_id: u32) {
        let mut engine = self.engine.lock().await;
        let _ = engine.on_disconnect(conn_id).await;
        self.internal.borrow_mut().remove(conn_id);
    }
}

impl CloudflareTransport {
    fn route_send(&self, id: u32, message: String) -> bool {
        if self.websockets.borrow_mut().send(id, message.clone()) {
            return true;
        }
        self.internal.borrow_mut().send(id, message)
    }

    async fn execute_rpc_action_inner(&self, conn_id: u32, action: crate::nostr::rpc_machine::RpcAction, engine: &mut NostrEngine<CloudflareStorage>) -> Result<(), crate::common::NwcError> {
        match action {
            crate::nostr::rpc_machine::RpcAction::Subscribe(sub_id, filter) => {
                let req_msg = crate::nostr::ClientMessage::Req(sub_id, vec![filter]);
                let responses = engine.handle_typed(conn_id, req_msg).await;
                self.process_responses(responses, engine).await.map_err(|e| crate::common::NwcError::ProtocolError(e.to_string()))?;
            }
            crate::nostr::rpc_machine::RpcAction::Publish(event) => {
                let event_msg = crate::nostr::ClientMessage::Event(event);
                let responses = engine.handle_typed(conn_id, event_msg).await;
                self.process_responses(responses, engine).await.map_err(|e| crate::common::NwcError::ProtocolError(e.to_string()))?;
            }
            crate::nostr::rpc_machine::RpcAction::Unsubscribe(sub_id) => {
                let responses = engine.process_close(conn_id, sub_id).await;
                self.process_responses(responses, engine).await.map_err(|e| crate::common::NwcError::ProtocolError(e.to_string()))?;
            }
        }
        Ok(())
    }

    async fn load_connection_with_handler(&self, pubkey: &str, engine: &mut NostrEngine<CloudflareStorage>) -> Result<Option<u32>> {
        if let Some(id) = engine.load_by_pubkey(pubkey).await {
            let ws = self.websockets.borrow().find_by_id(id);
            if let Some(ws) = ws {
                let _ = self.wake_up_with_handler(&ws, engine).await;
                return Ok(Some(id));
            }

            let all_ws = self.websockets.borrow().get_all_websockets();
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
        let id = self.websockets.borrow().identify(ws)?;

        let _ = engine.load(id).await;

        self.websockets.borrow_mut().add_active(id, ws.clone());
        Some(id)
    }

    async fn accept_new_connection(&self) -> Result<Response> {
        let max_connections = self.env.var("MAX_CONNECTIONS")
            .and_then(|v| v.to_string().parse::<usize>().map_err(|e| Error::from(e.to_string())))
            .unwrap_or(20);

        if self.websockets.borrow().len() >= max_connections {
            return Response::error("Too Many Requests", 429);
        }

        let WebSocketPair { client, server } = WebSocketPair::new()?;

        {
            let websockets = self.websockets.borrow();
            websockets.accept_web_socket(&server);
        }

        let mut engine = self.engine.lock().await;
        let connection_id = self.websockets.borrow().next_id().await;
        let responses = engine.on_connect(connection_id).await;

        self.websockets.borrow_mut().register_active(connection_id, server);

        self.process_responses(responses, &mut engine).await?;

        Ok(Response::from_websocket(client)?)
    }

    async fn handle_disconnect(&self, ws: &WebSocket) -> Result<()> {
        let mut engine = self.engine.lock().await;
        if let Some(id) = self.wake_up_with_handler(ws, &mut engine).await {
            let responses = engine.on_terminate(id).await;
            self.process_responses(responses, &mut engine).await?;
            self.websockets.borrow_mut().remove(id);
        }
        Ok(())
    }

    async fn process_responses(&self, responses: Vec<EngineResponse>, engine: &mut NostrEngine<CloudflareStorage>) -> Result<()> {
        for resp in responses {
            match resp {
                EngineResponse::Send { recipient_id, message } => {
                    self.route_send(recipient_id, message.to_json());
                }
                EngineResponse::WakeUp { connection_id } => {
                    let ws = self.websockets.borrow().find_by_id(connection_id);
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