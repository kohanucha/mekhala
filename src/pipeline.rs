use crate::domain::{Event, Filter, Limits};
use crate::relay::{RelayMessage, ClientMessage};
use crate::router::Router;
use crate::protocol::NwcProtocol;
use crate::platform::Platform;
use crate::ConnectionState;
use lru::LruCache;
use std::cell::RefCell;
use worker::*;

/// EventPipeline orchestrates the processing of incoming Nostr messages.
/// It handles verification, caching, and routing.
pub struct EventPipeline<'a> {
    pub state: &'a State,
    pub router: &'a Router,
    pub verification_cache: &'a RefCell<LruCache<String, Result<(), String>>>,
}

impl<'a> EventPipeline<'a> {
    pub fn new(
        state: &'a State,
        router: &'a Router,
        verification_cache: &'a RefCell<LruCache<String, Result<(), String>>>,
    ) -> Self {
        Self {
            state,
            router,
            verification_cache,
        }
    }

    /// Unified dispatcher for incoming client messages.
    pub async fn dispatch(
        &self,
        ws: &WebSocket,
        conn_state: &mut ConnectionState,
        msg: ClientMessage,
    ) -> Result<()> {
        match msg {
            ClientMessage::Event(e) => self.handle_event(ws, conn_state, e).await,
            ClientMessage::Req(id, f) => self.handle_req(ws, conn_state, id, f).await,
            ClientMessage::Close(id) => {
                self.router.unsubscribe(self.state, ws, Some(id), conn_state)
            }
        }
    }

    /// Process an incoming EVENT.
    pub async fn handle_event(
        &self,
        ws: &WebSocket,
        conn_state: &mut ConnectionState,
        event: Event,
    ) -> Result<()> {
        // 1. Verify NWC-specific protocol rules
        if let Err(e) = NwcProtocol::validate_event(&event) {
            let _ = ws.send_with_str(&RelayMessage::Ok(event.id, false, e.to_string()).to_json());
            return Ok(());
        }

        // 2. Verify Nostr cryptographic integrity (leveraging the cache)
        if let Err(reason) = self.verify_with_cache(&event, &conn_state.limits) {
            let _ = ws.send_with_str(&RelayMessage::Ok(event.id, false, reason).to_json());
            return Ok(());
        }

        // 3. State-dependent logic: Cache Info Events
        if event.kind == crate::protocol::KIND_NWC_INFO {
            conn_state.info_event = Some(event.clone());
            self.router.update_info_event(ws, event.clone());
            ws.serialize_attachment(conn_state)?;
        }

        // 4. Acknowledge and Broadcast
        let _ = ws.send_with_str(&RelayMessage::Ok(event.id.clone(), true, "".into()).to_json());
        
        self.router.broadcast(&event)
    }

    /// Process an incoming REQ.
    pub async fn handle_req(
        &self,
        ws: &WebSocket,
        conn_state: &mut ConnectionState,
        sub_id: String,
        filters: Vec<Filter>,
    ) -> Result<()> {
        // 1. Apply Protocol Limits
        if conn_state.subscriptions.len() >= 20 && !conn_state.subscriptions.contains_key(&sub_id) {
            let _ = ws.send_with_str(
                &RelayMessage::Closed(sub_id, "rate-limited: max 20 subscriptions".into()).to_json(),
            );
            return Ok(());
        }

        // 2. Generic filter limit check
        if filters.iter().any(|f| !f.is_valid(&conn_state.limits)) {
            let msg = "rejected: filter too broad or too many items";
            let _ = ws.send_with_str(&RelayMessage::Closed(sub_id, msg.into()).to_json());
            return Ok(());
        }

        // 3. Enforce NWC protocol narrowing and kinds
        if filters.iter().any(|f| !NwcProtocol::validate_filter(f)) {
            let msg = "restricted: NIP-47 subscriptions must be narrowed by author, p-tag, or e-tag and use NWC kinds";
            let _ = ws.send_with_str(&RelayMessage::Closed(sub_id, msg.into()).to_json());
            return Ok(());
        }

        // 4. Register Subscription in Router
        self.router.subscribe(self.state, ws, sub_id.clone(), filters.clone(), conn_state)?;

        // 5. Serve cached Info Events from other connections (Fast Path!)
        self.serve_cached_info_events(ws, &sub_id, &filters).await?;

        let _ = ws.send_with_str(&RelayMessage::Eose(sub_id).to_json());
        Ok(())
    }

    /// Verify an event using the in-memory LRU cache.
    fn verify_with_cache(&self, event: &Event, limits: &Limits) -> Result<(), String> {
        let mut cache = self.verification_cache.borrow_mut();
        if let Some(res) = cache.get(&event.id) {
            return res.clone();
        }

        let res = event
            .verify(Platform::now(), limits)
            .map_err(|e| e.to_string());
        
        cache.put(event.id.clone(), res.clone());
        res
    }

    /// Retrieves matching info events from the Router's in-memory cache and serves them.
    async fn serve_cached_info_events(&self, ws: &WebSocket, sub_id: &str, filters: &[Filter]) -> Result<()> {
        let is_requesting_info = filters
            .iter()
            .any(|f| f.kinds.as_ref().map_or(false, |k| k.contains(&crate::protocol::KIND_NWC_INFO)));

        if !is_requesting_info {
            return Ok(());
        }

        // Use the Router's high-leverage Speed Layer
        for info in self.router.get_matching_info_events(filters) {
            let _ = ws.send_with_str(
                &RelayMessage::Event(sub_id.to_string(), info).to_json(),
            );
        }
        Ok(())
    }
}
