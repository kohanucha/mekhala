use crate::cloudflare::config::CloudflareConfig;
use crate::cloudflare::connection::{
    admit, attachment_of, Admission, AdmissionLimits, Attachment, ConnectionRegistry,
};
use crate::cloudflare::create_cors_response;
use crate::cloudflare::identity::{self, ClientIdentity};
use crate::log_debug;
use crate::log_error;
use crate::log_info;
use crate::log_warn;
use crate::nostr::engine::{EngineResponse, NostrEngine};
use crate::nostr::wallet_registry::Storage;
use crate::nostr::Limits;
use crate::util::short;
use futures::lock::Mutex;
use js_sys::{Object, Reflect};
use serde::Serialize;
use serde_wasm_bindgen::Serializer;
use std::cell::RefCell;
use wasm_bindgen::JsValue;
use worker::{
    durable_object, DurableObject, Env, Error, ListOptions, Request, Response, Result,
    ScheduledTime, State, WebSocket, WebSocketIncomingMessage, WebSocketPair,
};

/// How often a socket's attachment is refreshed with its last-seen time.
/// The attachment is the only record a hibernated socket leaves behind, so
/// the idle sweep can underestimate a connection's real age by at most this.
const ATTACHMENT_REFRESH_SECONDS: u64 = 30;

/// Storage reads allowed per eviction attempt. Deciding whether a candidate
/// is evictable costs one read, and a full relay must not turn every connect
/// attempt into a fan-out of reads.
const MAX_EVICTION_CHECKS: usize = 3;

pub struct CloudflareStorage {
    storage: worker::Storage,
}

#[async_trait::async_trait(?Send)]
impl Storage for CloudflareStorage {
    async fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.storage.get(key).await.ok().flatten()
    }
    async fn put_batch(
        &self,
        entries: std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<(), crate::nostr::StorageError> {
        // See docs/put_multiple_raw.md for why we use put_multiple_raw + Object.
        let obj = Object::new();
        for (k, v) in &entries {
            let key = JsValue::from(k.as_str());
            if let Ok(val) = v.serialize(&Serializer::json_compatible()) {
                let _ = Reflect::set(&obj, &key, &val);
            }
        }
        self.storage
            .put_multiple_raw(obj)
            .await
            .map_err(|e| crate::nostr::StorageError::from(e.to_string()))?;
        Ok(())
    }
    async fn delete_batch(&self, keys: Vec<String>) {
        for k in &keys {
            if let Err(e) = self.storage.delete(k).await {
                log_warn!("delete_batch: delete for '{}' failed: {}", k, e);
            }
        }
    }
    async fn list_prefix(&self, prefix: &str) -> Vec<(String, serde_json::Value)> {
        let map = match self
            .storage
            .list_with_options(ListOptions::new().prefix(prefix))
            .await
        {
            Ok(map) => map,
            Err(e) => {
                log_warn!("list_prefix('{}') failed: {}", prefix, e);
                return Vec::new();
            }
        };

        let mut entries = Vec::new();
        let iterator = map.entries();
        while let Ok(step) = iterator.next() {
            if step.done() {
                break;
            }
            let pair: js_sys::Array = step.value().into();
            let Some(key) = pair.get(0).as_string() else {
                continue;
            };
            let Ok(value) = serde_wasm_bindgen::from_value::<serde_json::Value>(pair.get(1)) else {
                log_warn!("list_prefix('{}'): skipping unreadable '{}'", prefix, key);
                continue;
            };
            entries.push((key, value));
        }
        entries
    }
}

struct IdCounterState {
    current_id: u32,
    id_limit: u32,
}

#[durable_object]
/// Durable Object that carries the relay: every WebSocket connection is
/// upgraded to this object, which routes client requests to the wallet
/// and replies back. One object instance per region, named `"GLOBAL"`.
///
/// Holds the `NostrEngine` (relay logic, subscription registry, and
/// state persistence), the live `ConnectionRegistry`, and the connection
/// ID counter.
pub struct CloudflareTransport {
    config: CloudflareConfig,
    state: State,
    engine: Mutex<NostrEngine<CloudflareStorage>>,
    connections: RefCell<ConnectionRegistry>,
    id_counter: Mutex<IdCounterState>,
}

impl DurableObject for CloudflareTransport {
    fn new(state: State, env: Env) -> Self {
        let config = CloudflareConfig::from_env(&env);
        let storage = CloudflareStorage {
            storage: state.storage(),
        };
        let limits = Limits {
            max_filter_items: config.max_filter_items,
            max_event_tags: config.max_event_tags,
            max_routing_keys_per_connection: config.max_routing_keys_per_connection,
            max_info_events: config.max_info_events,
            info_ttl_seconds: config.info_ttl_seconds,
            ..Limits::new(
                config.max_content_length,
                config.max_subscriptions_per_connection,
            )
        };
        let engine = NostrEngine::new_with_storage(storage, limits, crate::util::now);

        Self {
            config,
            state,
            engine: Mutex::new(engine),
            connections: RefCell::new(ConnectionRegistry::new()),
            id_counter: Mutex::new(IdCounterState {
                current_id: 0,
                id_limit: 0,
            }),
        }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let identity = identity::from_headers(&req);
        self.accept_new_connection(identity).await
    }

    /// Runs the idle sweep and keeps it scheduled. Alarms are one-shot, so
    /// every run arms the next one for as long as sockets exist.
    async fn alarm(&self) -> Result<Response> {
        log_debug!("→ alarm handler invoked");
        self.sweep_idle_connections().await;
        self.sweep_info_cache().await;
        self.rearm_sweep().await;
        Response::empty()
    }

    async fn websocket_message(
        &self,
        websocket: WebSocket,
        message: WebSocketIncomingMessage,
    ) -> Result<()> {
        log_debug!("→ websocket_message handler invoked");
        if !self.admit_incoming_frame(&websocket) {
            return Ok(());
        }
        if let WebSocketIncomingMessage::String(text) = message {
            if text.len() > 131072 {
                log_warn!("message too large: {} bytes", text.len());
                let _ = websocket.send_with_str(
                    crate::nostr::RelayMessage::Notice("message too large".into()).to_json(),
                );
                return Ok(());
            }

            let parsed = crate::nostr::ClientMessage::from_json(&text);

            match &parsed {
                Ok(crate::nostr::ClientMessage::Event(event)) => {
                    let mut engine = self.engine.lock().await;
                    let Some(connection_id) =
                        self.wake_up_with_handler(&websocket, &mut engine).await
                    else {
                        log_error!(
                            "connection not found for incoming EVENT, sending reconnect notice"
                        );
                        let _ = websocket.send_with_str(
                            crate::nostr::RelayMessage::Notice(
                                "connection lost: please reconnect".into(),
                            )
                            .to_json(),
                        );
                        return Ok(());
                    };

                    log_info!(
                        "← conn={} RECV EVENT kind={} pk={} id={}",
                        connection_id,
                        event.kind,
                        short(&event.pubkey, 8),
                        short(&event.id, 8)
                    );

                    match engine.validate_event(event) {
                        Ok(()) => {
                            log_debug!(
                                "✓ EVENT accepted kind={} pk={} id={}",
                                event.kind,
                                short(&event.pubkey, 8),
                                short(&event.id, 8)
                            );
                            let ok_msg = crate::nostr::RelayMessage::Ok(
                                event.id.clone(),
                                true,
                                String::new(),
                            )
                            .to_json();
                            log_info!("→ conn={} SEND {}", connection_id, ok_msg);
                            let _ = websocket.send_with_str(ok_msg);
                            let responses = engine
                                .route_verified_event(connection_id, event.clone())
                                .await;
                            self.process_responses(responses, &mut engine).await?;
                        }
                        Err((event_id, error_msg)) => {
                            log_warn!("✗ EVENT rejected id={}: {}", short(&event_id, 8), error_msg);
                            let fail_msg =
                                crate::nostr::RelayMessage::Ok(event_id, false, error_msg)
                                    .to_json();
                            log_info!("→ conn={} SEND {}", connection_id, fail_msg);
                            let _ = websocket.send_with_str(&fail_msg);
                        }
                    }
                }
                _ => {
                    let mut engine = self.engine.lock().await;
                    let Some(connection_id) =
                        self.wake_up_with_handler(&websocket, &mut engine).await
                    else {
                        log_error!(
                            "connection not found for incoming message, sending reconnect notice"
                        );
                        let _ = websocket.send_with_str(
                            crate::nostr::RelayMessage::Notice(
                                "connection lost: please reconnect".into(),
                            )
                            .to_json(),
                        );
                        return Ok(());
                    };

                    let responses = match parsed {
                        Ok(crate::nostr::ClientMessage::Req(sub_id, filters)) => {
                            log_info!("← conn={} RECV REQ sub={}", connection_id, sub_id);
                            for (i, f) in filters.iter().enumerate() {
                                let kinds = f
                                    .kinds
                                    .as_ref()
                                    .map_or_else(|| "any".into(), |k| format!("{k:?}"));
                                let auths = f.authors.as_ref().map_or_else(String::new, |a| {
                                    a.iter().map(|s| short(s, 8)).collect::<Vec<_>>().join(",")
                                });
                                let pts = f.p_tags.as_ref().map_or_else(String::new, |p| {
                                    p.iter().map(|s| short(s, 8)).collect::<Vec<_>>().join(",")
                                });
                                log_info!(
                                    "  filter[{}]: kinds={} authors={} #p={}",
                                    i,
                                    kinds,
                                    auths,
                                    pts
                                );
                            }
                            engine.handle_req(connection_id, sub_id, filters).await
                        }
                        Ok(crate::nostr::ClientMessage::Close(sub_id)) => {
                            log_info!("← conn={} RECV CLOSE sub={}", connection_id, sub_id);
                            engine.process_close(connection_id, sub_id).await
                        }
                        Ok(crate::nostr::ClientMessage::Event(_)) => unreachable!(),
                        Err(e) => {
                            log_error!("✗ parse failed: {}", e);
                            if let Some(crate::nostr::nip_01::PartialClientMessage::Event(id)) =
                                crate::nostr::nip_01::PartialClientMessage::from_json(&text)
                            {
                                vec![crate::nostr::engine::EngineResponse::send(
                                    connection_id,
                                    crate::nostr::RelayMessage::Ok(
                                        id,
                                        false,
                                        format!("parse failed: {e}"),
                                    ),
                                )]
                            } else {
                                vec![crate::nostr::engine::EngineResponse::send(
                                    connection_id,
                                    crate::nostr::RelayMessage::Notice(format!(
                                        "parse failed: {e}"
                                    )),
                                )]
                            }
                        }
                    };
                    self.process_responses(responses, &mut engine).await?;
                }
            }

            Ok(())
        } else {
            let _engine = self.engine.lock().await;
            log_warn!("binary message not supported");
            let _ = websocket.send_with_str(
                crate::nostr::RelayMessage::Notice("binary not supported".into()).to_json(),
            );
            Ok(())
        }
    }

    async fn websocket_close(
        &self,
        ws: WebSocket,
        code: usize,
        reason: String,
        was_clean: bool,
    ) -> Result<()> {
        log_debug!(
            "→ websocket_close handler invoked code={} reason={} clean={}",
            code,
            reason,
            was_clean
        );
        self.handle_disconnect(&ws).await
    }

    async fn websocket_error(&self, ws: WebSocket, error: Error) -> Result<()> {
        log_debug!("→ websocket_error handler invoked err={}", error);
        self.handle_disconnect(&ws).await
    }
}

impl CloudflareTransport {
    /// Charges one inbound frame against the connection's message budget.
    ///
    /// Every frame costs something once it is handled: a `REQ` re-reads and
    /// rewrites the connection's routing index (up to
    /// `MAX_ROUTING_KEYS_PER_CONNECTION` keys), an `EVENT` pays a signature
    /// check, and a `CLOSE` used to rewrite the whole snapshot even when it
    /// named a subscription that was not there. Without a budget one client can
    /// spend the single Durable Object's time — which every connection of the
    /// region shares — on any of those.
    ///
    /// Returns `false` when the budget is spent and the socket has been asked
    /// to close, so the caller stops before parsing or storage work.
    fn admit_incoming_frame(&self, websocket: &WebSocket) -> bool {
        let Some(id) = self.connections.borrow().identify(websocket) else {
            return true;
        };
        let decision = self.connections.borrow_mut().admit_message(
            id,
            crate::util::now(),
            self.config.message_rate_limit_per_minute,
        );
        let crate::util::window::Admission::Limited { retry_after } = decision else {
            return true;
        };

        log_warn!(
            "conn={} exceeded {} messages in a window; closing it",
            id,
            self.config.message_rate_limit_per_minute
        );
        let _ = websocket.send_with_str(
            crate::nostr::RelayMessage::Notice(format!("rate limited, retry in {retry_after}s"))
                .to_json(),
        );
        let _ = websocket.close(Some(1008), Some("message rate limit"));
        false
    }

    /// Keeps the cached wallet info family bounded and removes keys that no
    /// current code writes. Runs on the same alarm as the idle sweep: both
    /// keep storage bounded by time, and the cache is only read while sockets
    /// exist.
    async fn sweep_info_cache(&self) {
        let mut engine = self.engine.lock().await;

        // One-way cleanup of key families earlier versions wrote (`info-p:`
        // aliases, Lightning Address `rate:` counters). This runs regardless of
        // the cache bounds below because those bounds only control
        // `info:`/`infoid:`; leaving the retired keys behind would leak storage
        // forever.
        let retired = engine.prune_retired_keys().await;
        if retired > 0 {
            log_info!("storage: {} retired keys removed", retired);
        }

        let config = &self.config;
        if config.info_ttl_seconds == 0 && config.max_info_events == 0 {
            return;
        }
        let removed = engine.prune_info_cache().await;
        if removed > 0 {
            log_info!("info cache: {} entries swept", removed);
        }
    }

    async fn allocate_id(&self) -> u32 {
        let mut counter = self.id_counter.lock().await;
        let current = counter.current_id;
        let limit = counter.id_limit;

        if current >= limit {
            let storage = self.state.storage();
            let last_limit = storage
                .get::<u32>("id_counter")
                .await
                .ok()
                .flatten()
                .unwrap_or(0);
            let now = crate::util::now() as u32;
            let start = std::cmp::max(last_limit, now);
            let new_limit = start + 1000;
            let _ = storage.put("id_counter", new_limit).await;
            counter.current_id = start + 1;
            counter.id_limit = new_limit;
            counter.current_id
        } else {
            counter.current_id = current + 1;
            counter.current_id
        }
    }

    fn route_send(&self, id: u32, message: &str) -> bool {
        log_info!("→ conn={} SEND {}", id, message);
        if self.connections.borrow().send(id, message) {
            true
        } else {
            log_warn!("✗ send failed: conn={} not found", id);
            false
        }
    }

    async fn wake_up_with_handler(
        &self,
        ws: &WebSocket,
        engine: &mut NostrEngine<CloudflareStorage>,
    ) -> Option<u32> {
        let Some(id) = self.connections.borrow().identify(ws) else {
            log_warn!("wake_up: identify failed for ws");
            return None;
        };

        let _ = engine.load(id).await;

        self.connections
            .borrow_mut()
            .touch(ws, id, crate::util::now(), ATTACHMENT_REFRESH_SECONDS);
        self.connections.borrow_mut().insert_active(id, ws.clone());
        log_debug!("↻ wake conn={}", id);
        Some(id)
    }

    async fn accept_new_connection(&self, identity: ClientIdentity) -> Result<Response> {
        let limits = AdmissionLimits {
            max_connections: self.config.max_connections,
            max_per_ip: self.config.max_connections_per_ip,
            max_per_asn: self.config.max_connections_per_asn,
        };
        let now = crate::util::now();
        let sockets = self.state.get_websockets();
        let existing: Vec<Attachment> = sockets.iter().filter_map(attachment_of).collect();
        let candidate = Attachment {
            id: 0,
            ip_tag: identity.ip_tag,
            asn: identity.asn,
            last_seen: now,
        };

        // Room is decided before an id is allocated: a refused connection
        // must not burn one.
        let mut freed = 0usize;
        match admit(&existing, sockets.len(), &candidate, &limits) {
            Admission::Accept => {}
            Admission::IpAtCapacity => {
                log_warn!(
                    "✗ connection rejected: address limit {} reached",
                    limits.max_per_ip
                );
                return Response::error("Too Many Requests", 429);
            }
            Admission::AsnAtCapacity => {
                log_warn!(
                    "✗ connection rejected: network limit {} reached",
                    limits.max_per_asn
                );
                return Response::error("Too Many Requests", 429);
            }
            Admission::Full => {
                if self.reclaim_idle_slot(&existing, now).await? {
                    // The evicted socket stays in `get_websockets()` until
                    // its close propagates, so its slot is counted as freed
                    // rather than read back as still occupied.
                    freed = 1;
                } else {
                    log_warn!(
                        "✗ connection rejected: max={} connections reached",
                        limits.max_connections
                    );
                    return Response::error("Too Many Requests", 429);
                }
            }
        }

        let WebSocketPair { client, server } = WebSocketPair::new()?;
        let connection_id = self.allocate_id().await;
        let attachment = Attachment {
            id: connection_id,
            ..candidate
        };

        {
            let mut connections = self.connections.borrow_mut();
            // Re-read the live count: an await above may have admitted
            // another connection.
            if connections.len(&self.state) >= limits.max_connections.saturating_add(freed) {
                log_warn!(
                    "✗ connection rejected: max={} connections reached",
                    limits.max_connections
                );
                return Response::error("Too Many Requests", 429);
            }
            connections.accept_and_register(&self.state, &attachment, &server);
        }

        {
            let mut engine = self.engine.lock().await;
            let responses = engine.on_connect(connection_id).await;

            log_info!(
                "+ conn={} accepted count={}/{}",
                connection_id,
                self.connections.borrow().len(&self.state),
                limits.max_connections
            );

            self.process_responses(responses, &mut engine).await?;
        }

        self.arm_sweep().await;

        Response::from_websocket(client)
    }

    /// Closes the longest-idle connection that holds no subscription to make
    /// room for a new one; returns whether a slot was freed.
    ///
    /// A connection with subscriptions is never evicted: that is exactly the
    /// client the relay exists for. Candidates are considered oldest first
    /// and only past `EVICT_MIN_IDLE_SECONDS`, so a client that just
    /// connected still has time to subscribe.
    async fn reclaim_idle_slot(&self, existing: &[Attachment], now: u64) -> Result<bool> {
        let min_idle = u64::try_from(self.config.evict_min_idle_seconds).unwrap_or(u64::MAX);
        // Order and eligibility both use the merged activity time: an
        // attachment can lag the memory record by up to the refresh interval,
        // and sorting on it alone could hide the real oldest candidate.
        let mut candidates: Vec<(u64, u32)> = existing
            .iter()
            .map(|attachment| (self.idle_since(attachment), attachment.id))
            .collect();
        candidates.sort_unstable();

        for (seen, id) in candidates.into_iter().take(MAX_EVICTION_CHECKS) {
            if now.saturating_sub(seen) < min_idle {
                // Sorted oldest first: every remaining candidate is younger.
                break;
            }

            let mut engine = self.engine.lock().await;
            if engine.has_subscriptions(id).await {
                continue;
            }

            let ws = self
                .state
                .get_websockets()
                .into_iter()
                .find(|ws| attachment_of(ws).is_some_and(|attachment| attachment.id == id));
            let responses = engine.on_terminate(id).await;
            self.connections.borrow_mut().remove(id);
            self.process_responses(responses, &mut engine).await?;
            drop(engine);

            if let Some(ws) = ws {
                let _ = ws.close(Some(1000), Some("connection slot reclaimed"));
            }
            log_info!(
                "- conn={} evicted: idle {}s with no subscriptions",
                id,
                now.saturating_sub(seen)
            );
            return Ok(true);
        }

        Ok(false)
    }

    /// Closes every connection that has been idle longer than
    /// `IDLE_TIMEOUT_SECONDS` and holds no subscription, then cleans up its
    /// state. A disabled timeout (`0`) skips the work entirely.
    async fn sweep_idle_connections(&self) {
        let idle_timeout = u64::try_from(self.config.idle_timeout_seconds).unwrap_or(u64::MAX);
        if idle_timeout == 0 {
            return;
        }

        let now = crate::util::now();
        let mut closed = 0usize;

        for ws in self.state.get_websockets() {
            let Some(attachment) = attachment_of(&ws) else {
                continue;
            };
            let seen = self.idle_since(&attachment);
            if now.saturating_sub(seen) < idle_timeout {
                continue;
            }

            let mut engine = self.engine.lock().await;
            if engine.has_subscriptions(attachment.id).await {
                continue;
            }

            let responses = engine.on_terminate(attachment.id).await;
            self.connections.borrow_mut().remove(attachment.id);
            let _ = self.process_responses(responses, &mut engine).await;
            drop(engine);

            log_info!(
                "- conn={} idle {}s with no subscriptions: closing",
                attachment.id,
                now.saturating_sub(seen)
            );
            if let Err(e) = ws.close(Some(1000), Some("idle timeout")) {
                log_warn!("close failed for conn={}: {}", attachment.id, e);
            }
            closed += 1;
        }

        if closed > 0 {
            log_info!("idle sweep closed {} connection(s)", closed);
        }
    }

    /// Most recent activity known for a connection, merging this instance's
    /// record (accurate while awake) with the attachment (written before
    /// hibernation, refreshed at most every `ATTACHMENT_REFRESH_SECONDS`).
    fn idle_since(&self, attachment: &Attachment) -> u64 {
        let observed = self.connections.borrow().last_seen(attachment.id);
        observed.map_or(attachment.last_seen, |seen| seen.max(attachment.last_seen))
    }

    /// Arms the idle sweep if it is enabled and not already scheduled.
    async fn arm_sweep(&self) {
        let Some(interval_ms) = self.sweep_interval_ms() else {
            return;
        };
        let storage = self.state.storage();
        match storage.get_alarm().await {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(e) => {
                log_warn!("get_alarm failed, re-arming anyway: {}", e);
            }
        }
        if let Err(e) = storage.set_alarm(ScheduledTime::from(interval_ms)).await {
            log_warn!("set_alarm failed: {}", e);
        }
    }

    /// Arms the next sweep, unless there is nothing left to sweep — the next
    /// accepted connection arms the loop again.
    async fn rearm_sweep(&self) {
        let Some(interval_ms) = self.sweep_interval_ms() else {
            return;
        };
        if self.state.get_websockets().is_empty() {
            return;
        }
        if let Err(e) = self
            .state
            .storage()
            .set_alarm(ScheduledTime::from(interval_ms))
            .await
        {
            log_warn!("set_alarm failed: {}", e);
        }
    }

    /// Delay between sweeps, or `None` when the sweep is switched off.
    fn sweep_interval_ms(&self) -> Option<i64> {
        if self.config.idle_timeout_seconds == 0 || self.config.connection_sweep_seconds == 0 {
            return None;
        }
        let seconds = i64::try_from(self.config.connection_sweep_seconds).unwrap_or(i64::MAX);
        Some(seconds.saturating_mul(1000))
    }

    async fn handle_disconnect(&self, ws: &WebSocket) -> Result<()> {
        let mut engine = self.engine.lock().await;
        if let Some(id) = self.wake_up_with_handler(ws, &mut engine).await {
            log_info!("- conn={} disconnected", id);
            let responses = engine.on_terminate(id).await;
            self.process_responses(responses, &mut engine).await?;
            self.connections.borrow_mut().remove(id);
        }
        Ok(())
    }

    async fn process_responses(
        &self,
        responses: Vec<EngineResponse>,
        engine: &mut NostrEngine<CloudflareStorage>,
    ) -> Result<()> {
        for resp in responses {
            match resp {
                EngineResponse::Send {
                    recipient_id,
                    message,
                } => {
                    self.route_send(recipient_id, &message.to_json());
                }
                EngineResponse::WakeUp { connection_id } => {
                    let ws = self
                        .connections
                        .borrow()
                        .find_ws_by_id(&self.state, connection_id);
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
    let config = CloudflareConfig::from_env(env);
    // Admission limits are enforced inside the Durable Object, which cannot
    // read the edge's view of the client, so stamp it on the forwarded
    // request here — replacing anything the client sent under those names.
    // Headers of a request that arrived from the network are immutable, so the
    // request is re-created before it is touched.
    let mut req = req.clone_mut()?;
    identity::stamp(&mut req)?;
    crate::cloudflare::get_durable_stub(env, config.wallet_region.as_deref())?
        .fetch_with_request(req)
        .await
}

pub fn create_response(info: &serde_json::Value, content_type: &str) -> Result<Response> {
    let mut response = create_cors_response(Response::from_json(info)?)?;
    response.headers_mut().set("Content-Type", content_type)?;
    Ok(response)
}
