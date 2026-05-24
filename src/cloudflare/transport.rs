use std::rc::Rc;
use futures::lock::Mutex;
use worker::*;
use crate::cloudflare::connection::CloudflareConnectionTransport;
use crate::cloudflare::create_cors_response;
use crate::cloudflare::id_allocator::{DoIdBatchStore, IdAllocator};
use crate::cloudflare::kv::CloudflareKvStore;
use crate::cloudflare::rpc_context::CloudflareRpcContext;
use crate::cloudflare::storage::CloudflareStorage;
use crate::nostr::connection::ConnectionManager;
use crate::nostr::engine::NostrEngine;
use crate::util::short;
use crate::log_info;
use crate::log_debug;
use crate::log_warn;
use crate::log_error;

#[durable_object]
pub struct CloudflareTransport {
    env: Env,
    engine: Rc<Mutex<NostrEngine<CloudflareStorage>>>,
    manager: Rc<Mutex<ConnectionManager<CloudflareConnectionTransport>>>,
    id_allocator: Rc<IdAllocator<DoIdBatchStore>>,
    kv: CloudflareKvStore,
    rpc_ctx: CloudflareRpcContext,
}

impl DurableObject for CloudflareTransport {
    fn new(state: State, env: Env) -> Self {
        let state = Rc::new(state);
        let storage = CloudflareStorage::new(state.storage());
        let max_content_length = env.var("MAX_CONTENT_LENGTH")
            .and_then(|v| v.to_string().parse::<usize>().map_err(|e| Error::from(e.to_string())))
            .unwrap_or(65536);
        let engine = NostrEngine::new_with_storage(storage, max_content_length, crate::util::now);
        let kv = CloudflareKvStore::new(env.kv("MEKHALA_NWC_KV").expect("MEKHALA_NWC_KV not configured"));

        let transport = CloudflareConnectionTransport::new(Rc::clone(&state));
        let manager = ConnectionManager::new(transport);
        let id_store = DoIdBatchStore::new(state.storage());
        let id_allocator = IdAllocator::new(id_store);

        let engine = Rc::new(Mutex::new(engine));
        let manager = Rc::new(Mutex::new(manager));
        let id_allocator = Rc::new(id_allocator);
        let rpc_ctx = CloudflareRpcContext::new(
            Rc::clone(&engine),
            Rc::clone(&manager),
            Rc::clone(&id_allocator),
        );

        Self {
            env,
            engine,
            manager,
            id_allocator,
            kv,
            rpc_ctx,
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
        log_debug!("→ websocket_message handler invoked");
        let text = match message {
            WebSocketIncomingMessage::String(t) => t,
            _ => {
                log_warn!("binary message not supported");
                let _ = websocket.send_with_str(&crate::nostr::RelayMessage::Notice("binary not supported".into()).to_json());
                return Ok(());
            }
        };

        if text.len() > 131072 {
            log_warn!("message too large: {} bytes", text.len());
            let _ = websocket.send_with_str(&crate::nostr::RelayMessage::Notice("message too large".into()).to_json());
            return Ok(());
        }

        let connection_id = match self.manager.lock().await.identify(&websocket) {
            Some(id) => id,
            None => {
                log_error!("connection not found, sending reconnect notice");
                let _ = websocket.send_with_str(&crate::nostr::RelayMessage::Notice("connection lost: please reconnect".into()).to_json());
                return Ok(());
            }
        };

        let mut engine = self.engine.lock().await;
        let engine_ref = &mut *engine;

        self.manager.lock().await.wake_and_load(connection_id, engine_ref).await;

        let parsed = crate::nostr::ClientMessage::from_json(&text);
        match &parsed {
            Ok(crate::nostr::ClientMessage::Event(event)) => {
                log_info!("← conn={} RECV EVENT kind={} pk={} id={}", connection_id, event.kind, short(&event.pubkey, 8), short(&event.id, 8));
            }
            Ok(crate::nostr::ClientMessage::Req(sub_id, filters)) => {
                log_info!("← conn={} RECV REQ sub={}", connection_id, sub_id);
                for (i, f) in filters.iter().enumerate() {
                    let kinds = f.kinds.as_ref().map(|k| format!("{:?}", k)).unwrap_or_else(|| "any".into());
                    let auths = f.authors.as_ref().map(|a| a.iter().map(|s| short(s, 8)).collect::<Vec<_>>().join(",")).unwrap_or_else(|| "".into());
                    let pts = f.p_tags.as_ref().map(|p| p.iter().map(|s| short(s, 8)).collect::<Vec<_>>().join(",")).unwrap_or_else(|| "".into());
                    log_info!("  filter[{}]: kinds={} authors={} #p={}", i, kinds, auths, pts);
                }
            }
            Ok(crate::nostr::ClientMessage::Close(sub_id)) => {
                log_info!("← conn={} RECV CLOSE sub={}", connection_id, sub_id);
            }
            Err(parse_error) => {
                log_error!("✗ parse failed: {}", parse_error.message);
            }
        }

        let responses = match parsed {
            Ok(msg) => engine_ref.handle_typed(connection_id, msg).await,
            Err(parse_error) => parse_error.into_responses(connection_id),
        };

        self.manager.lock().await.dispatch(responses, engine_ref).await;
        Ok(())
    }

    async fn websocket_close(&self, ws: WebSocket, code: usize, reason: String, was_clean: bool) -> Result<()> {
        log_debug!("→ websocket_close handler invoked code={} reason={} clean={}", code, reason, was_clean);
        self.handle_disconnect(&ws).await
    }

    async fn websocket_error(&self, ws: WebSocket, error: Error) -> Result<()> {
        log_debug!("→ websocket_error handler invoked err={}", error);
        self.handle_disconnect(&ws).await
    }
}

#[async_trait::async_trait(?Send)]
impl crate::common::NwcTransport for CloudflareTransport {
    async fn get_wallet_info(&self, pubkey: &str) -> Option<crate::nostr::WalletInfo> {
        let mut engine = self.engine.lock().await;
        let conn_ids = engine.load_by_pubkey(pubkey).await;

        {
            let manager = self.manager.lock().await;
            for id in conn_ids {
                manager.try_activate(id);
            }
        }

        engine.get_wallet_info(pubkey).await
    }

    async fn execute_nwc_rpc(&self, request: crate::nostr::Event) -> Result<crate::nostr::Event, crate::common::NwcError> {
        crate::nostr::rpc_orchestrator::execute_nwc_rpc(&self.rpc_ctx, request).await
    }
}

impl CloudflareTransport {
    async fn accept_new_connection(&self) -> Result<Response> {
        let max_connections = self.env.var("MAX_CONNECTIONS")
            .and_then(|v| v.to_string().parse::<usize>().map_err(|e| Error::from(e.to_string())))
            .unwrap_or(20);

        let WebSocketPair { client, server } = WebSocketPair::new()?;

        // 1. Allocate ID without holding any borrow across await
        let connection_id = self.id_allocator.allocate().await;

        // 2. Accept + register atomically with count re-check inside lock
        {
            let mut manager = self.manager.lock().await;
            if manager.total_count() >= max_connections {
                log_warn!("✗ connection rejected: max={} connections reached", max_connections);
                return Response::error("Too Many Requests", 429);
            }
            manager.accept_and_register(connection_id, &server);
        }

        // 3. NOW safe to await — WS is accepted and in HashMap
        let mut engine = self.engine.lock().await;
        let engine_ref = &mut *engine;
        let responses = engine_ref.on_connect(connection_id).await;

        let response = Response::from_websocket(client)?;

        let mut manager = self.manager.lock().await;
        log_info!("+ conn={} accepted count={}/{}", connection_id, manager.total_count(), max_connections);
        manager.dispatch(responses, engine_ref).await;

        Ok(response)
    }

    async fn handle_disconnect(&self, ws: &WebSocket) -> Result<()> {
        let conn_id = self.manager.lock().await.identify(ws);

        if let Some(id) = conn_id {
            let mut engine = self.engine.lock().await;
            let engine_ref = &mut *engine;
            let mut manager = self.manager.lock().await;

            // Wake up if hibernated, then terminate
            manager.wake_and_load(id, engine_ref).await;

            log_info!("- conn={} disconnected", id);
            manager.on_terminate(id, engine_ref).await;
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
