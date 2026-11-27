use super::limits::MAX_SUB_ID_LENGTH;
use super::wallet_registry::{RegistryResponse, Storage, WalletRegistry};
use super::{Event, Filter, Limits, RelayMessage};
use crate::util::short;
use crate::{log_debug, log_info, log_warn};
use std::collections::{HashMap, HashSet};

/// An action the transport must perform after the engine handles a message.
#[derive(Debug, PartialEq, Eq)]
pub enum EngineResponse {
    /// Send a serialized relay message to the given connection.
    Send {
        recipient_id: u32,
        message: RelayMessage,
    },
    /// Hydrate a hibernating connection so its live state is loaded.
    WakeUp { connection_id: u32 },
}

impl EngineResponse {
    /// Convenience constructor for a delivery action.
    pub fn send(recipient_id: u32, message: RelayMessage) -> Self {
        Self::Send {
            recipient_id,
            message,
        }
    }

    /// Convenience constructor for a wake-up action.
    pub fn wake_up(connection_id: u32) -> Self {
        Self::WakeUp { connection_id }
    }
}

/// Relay state machine: validates decoded NIP-01 messages, routes events to
/// matching subscriptions through the [`WalletRegistry`], and maintains the
/// wallet-service info cache. Transport-agnostic — the Durable Object
/// transport decodes WebSocket frames and feeds them in.
pub struct NostrEngine<S: Storage> {
    registry: WalletRegistry<S>,
    limits: Limits,
    clock: fn() -> u64,
}

impl<S: Storage> NostrEngine<S> {
    /// Builds an engine over the given storage; `clock` supplies the current
    /// Unix time (injectable so tests can pin it).
    pub fn new_with_storage(storage: S, limits: Limits, clock: fn() -> u64) -> Self {
        Self {
            registry: WalletRegistry::new(storage, limits, clock),
            limits,
            clock,
        }
    }

    /// Validate an event without routing it. Returns `Ok(event_id)` if
    /// accepted, or `Err((event_id, error_message))` if rejected.
    pub fn validate_event(&self, event: &Event) -> Result<(), (String, String)> {
        let ts = (self.clock)();
        match event.kind {
            5 | 13194 | 23194..=23197 => event
                .verify(ts, &self.limits)
                .map_err(|e| (event.id.clone(), e.to_string())),
            _ => {
                log_warn!(
                    "event rejected: kind={} reason=kind not allowed",
                    event.kind
                );
                Err((event.id.clone(), "blocked: event kind not allowed".into()))
            }
        }
    }

    /// Route a pre-verified event. Does NOT send OK — the caller is responsible
    /// for sending OK immediately upon successful validation (per NIP-01).
    pub async fn route_verified_event(
        &mut self,
        connection_id: u32,
        event: Event,
    ) -> Vec<EngineResponse> {
        if event.kind == 13194 {
            log_info!("info cached: pk={}", short(&event.pubkey, 8));
            self.process_info_event(event.clone()).await;
        } else if event.kind == 5 {
            self.process_deletion_event(&event).await;
        }
        self.route_event(connection_id, event).await
    }

    /// Handles a `REQ`: rejects oversized subscriptions and overly broad
    /// filters with `CLOSED`, then subscribes and replays matching cached
    /// wallet info events followed by `EOSE`.
    pub async fn handle_req(
        &mut self,
        id: u32,
        sub_id: String,
        filters: Vec<Filter>,
    ) -> Vec<EngineResponse> {
        if sub_id.len() > MAX_SUB_ID_LENGTH {
            let message = RelayMessage::Closed(
                sub_id,
                format!("subscription id too long (max {MAX_SUB_ID_LENGTH} bytes)"),
            );
            return vec![EngineResponse::send(id, message)];
        }

        if filters.len() > self.limits.max_filters_per_req {
            let message = RelayMessage::Closed(
                sub_id,
                format!("too many filters (max {})", self.limits.max_filters_per_req),
            );
            return vec![EngineResponse::send(id, message)];
        }

        if filters.iter().any(|f| f.exceeds_item_limit(&self.limits)) {
            let message = RelayMessage::Closed(
                sub_id,
                format!(
                    "filter too large (max {} values per field)",
                    self.limits.max_filter_items
                ),
            );
            return vec![EngineResponse::send(id, message)];
        }

        if !filters.iter().all(Filter::has_hex_values) {
            let message = RelayMessage::Closed(
                sub_id,
                "invalid filter value: ids, authors, #p and #e take 64 lowercase hex characters"
                    .to_string(),
            );
            return vec![EngineResponse::send(id, message)];
        }

        if filters.iter().any(|f| !f.is_valid(&self.limits)) {
            let message = RelayMessage::Closed(sub_id, "filter too broad".to_string());
            return vec![EngineResponse::send(id, message)];
        }

        self.process_req(id, sub_id, filters).await
    }

    /// Registers a freshly accepted connection and returns any responses to
    /// emit; currently registration only, so the vector is empty (the relay
    /// sends nothing until the client issues `REQ`).
    pub async fn on_connect(&mut self, id: u32) -> Vec<EngineResponse> {
        log_debug!("connect conn={}", id);
        self.add_connection(id, HashMap::new()).await;
        Vec::new()
    }

    async fn process_info_event(&mut self, event: Event) {
        log_debug!("persist info: pk={}", short(&event.pubkey, 8));
        self.registry.cache_info(event).await;
    }

    async fn process_deletion_event(&mut self, event: &Event) {
        let author = &event.pubkey;
        let e_tag_ids: Vec<String> = event
            .tags
            .iter()
            .filter_map(|t| t.event_id().map(|s| s.to_string()))
            .collect();

        if !e_tag_ids.is_empty() {
            for event_id in &e_tag_ids {
                if self.registry.delete_info_by_event(event_id, author).await {
                    log_info!(
                        "info deleted by e-tag: pk={} event_id={}",
                        short(author, 8),
                        short(event_id, 8)
                    );
                }
            }
        } else {
            let k_tags: Vec<u64> = event.tags.iter().filter_map(|t| t.kind_value()).collect();

            if k_tags.is_empty() || k_tags.contains(&13194) {
                log_info!("info deleted for author: pk={}", short(author, 8));
                self.registry.delete_info(author).await;
            }
        }
    }

    /// Route event to subscribers without sending OK (caller already sent it).
    async fn route_event(&mut self, _connection_id: u32, event: Event) -> Vec<EngineResponse> {
        let mut responses = Vec::new();

        let registry_responses = self.registry.match_event(&event).await;
        log_debug!(
            "event kind={} pk={} → {} subscribers",
            event.kind,
            short(&event.pubkey, 8),
            registry_responses.len()
        );
        for resp in registry_responses {
            match resp {
                RegistryResponse::Send {
                    recipient_id,
                    sub_id,
                } => {
                    responses.push(EngineResponse::send(
                        recipient_id,
                        RelayMessage::Event(sub_id, event.clone()),
                    ));
                }
                RegistryResponse::WakeUp(recipient_id) => {
                    responses.push(EngineResponse::wake_up(recipient_id));
                }
            }
        }

        responses
    }

    async fn process_req(
        &mut self,
        id: u32,
        sub_id: String,
        filters: Vec<Filter>,
    ) -> Vec<EngineResponse> {
        let mut responses = Vec::new();
        if let Err(e) = self
            .registry
            .subscribe(id, sub_id.clone(), filters.clone())
            .await
        {
            log_warn!(
                "sub rejected: conn={} sub={}: {}",
                id,
                short(&sub_id, 16),
                e
            );
            return vec![EngineResponse::send(
                id,
                RelayMessage::Closed(sub_id, e.to_string()),
            )];
        }

        let global_limit = filters.iter().filter_map(|f| f.limit).min();

        // One read per wallet, however many filters name it. Each author has
        // one cached event, so distinct keys never replay the same event.
        let mut seen = HashSet::new();
        let pubkeys: Vec<String> = filters
            .iter()
            .flat_map(Filter::pubkeys)
            .filter(|pk| seen.insert(pk.clone()))
            .collect();
        for pk in pubkeys {
            if let Some(info_event) = self.registry.get_info(&pk).await {
                if filters.iter().any(|f| f.matches(&info_event)) {
                    log_debug!("info hit: pk={} sub={}", short(&pk, 8), short(&sub_id, 16));
                    responses.push(EngineResponse::send(
                        id,
                        RelayMessage::Event(sub_id.clone(), info_event),
                    ));
                }
            } else {
                log_debug!("info miss: pk={} sub={}", short(&pk, 8), short(&sub_id, 16));
            }
        }

        if let Some(limit) = global_limit {
            // NIP-01 `limit`: replay at most this many cached info events.
            // `responses` only holds those events at this point.
            let limit = usize::try_from(limit).unwrap_or(usize::MAX);
            responses.truncate(limit);
        }

        responses.push(EngineResponse::send(id, RelayMessage::Eose(sub_id)));
        responses
    }

    /// Ends a subscription and persists the change; returns no responses.
    pub async fn process_close(&mut self, id: u32, sub_id: String) -> Vec<EngineResponse> {
        log_debug!("close conn={} sub={}", id, short(&sub_id, 16));
        let _ = self.registry.unsubscribe(id, sub_id).await;
        Vec::new()
    }

    /// Final teardown: removes live index entries and persisted state.
    pub async fn on_terminate(&mut self, id: u32) -> Vec<EngineResponse> {
        log_debug!("terminate conn={}", id);
        self.registry.on_terminate(id).await;
        Vec::new()
    }

    /// Removes the stored state of connections whose socket is gone without
    /// a close event (see `WalletRegistry::prune_orphaned_connections`);
    /// returns how many were removed.
    pub async fn prune_orphaned_connections(
        &mut self,
        live: &std::collections::HashSet<u32>,
        interval: u64,
    ) -> usize {
        self.registry
            .prune_orphaned_connections(live, interval)
            .await
    }

    /// Drops cached wallet info events that are too old or beyond the cache
    /// cap; returns how many were removed. The idle sweep calls this so the
    /// cache cleans itself up while the object is awake.
    pub async fn prune_info_cache(&mut self) -> usize {
        self.registry.prune_info_cache().await
    }

    /// Removes keys that earlier versions wrote and nothing current reads
    /// (see `WalletRegistry::prune_retired_keys`); returns how many keys were
    /// deleted.
    pub async fn prune_retired_keys(&mut self) -> usize {
        self.registry.prune_retired_keys().await
    }

    /// Registers the given subscriptions for a connection, ignoring
    /// individual failures (best-effort restore path).
    pub async fn add_connection(&mut self, id: u32, subscriptions: HashMap<String, Vec<Filter>>) {
        for (sub_id, filters) in subscriptions {
            let _ = self.registry.subscribe(id, sub_id, filters).await;
        }
    }

    /// Restores the connection's subscriptions from storage into the live
    /// index; returns whether it has subscriptions afterwards.
    pub async fn load(&mut self, conn_id: u32) -> bool {
        self.registry.load(conn_id).await
    }

    /// Whether the connection holds at least one subscription right now,
    /// loading its persisted state first.
    pub async fn has_subscriptions(&mut self, conn_id: u32) -> bool {
        self.registry.has_subscriptions(conn_id).await
    }
}

#[cfg(test)]
#[path = "engine_test.rs"]
mod engine_test;
