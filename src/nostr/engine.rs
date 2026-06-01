use std::collections::HashMap;
use super::{Filter, Event, RelayMessage, ClientMessage, Limits};
use super::wallet_registry::{WalletRegistry, Storage, RegistryResponse};
use crate::util::short;
use crate::{log_info, log_debug, log_warn};

#[derive(Debug, PartialEq, Eq)]
pub enum EngineResponse {
    Send { recipient_id: u32, message: RelayMessage },
    WakeUp { connection_id: u32 },
}

impl EngineResponse {
    pub fn send(recipient_id: u32, message: RelayMessage) -> Self {
        EngineResponse::Send { recipient_id, message }
    }

    pub fn wake_up(connection_id: u32) -> Self {
        EngineResponse::WakeUp { connection_id }
    }
}

pub struct NostrEngine<S: Storage> {
    registry: WalletRegistry<S>,
    limits: Limits,
    clock: fn() -> u64,
}

impl<S: Storage> NostrEngine<S> {
    pub fn new_with_storage(storage: S, limits: Limits, clock: fn() -> u64) -> Self {
        Self {
            registry: WalletRegistry::new(storage, limits),
            limits,
            clock,
        }
    }

    pub async fn handle_typed(&mut self, connection_id: u32, message: ClientMessage) -> Vec<EngineResponse> {
        match message {
            ClientMessage::Event(event) => self.handle_event(connection_id, event).await,
            ClientMessage::Req(sub_id, filters) => self.handle_req(connection_id, sub_id, filters).await,
            ClientMessage::Close(sub_id) => self.process_close(connection_id, sub_id).await,
        }
    }

    /// Validate an event without routing it. Returns Ok(event_id) if accepted,
    /// or Err((event_id, error_message)) if rejected.
    pub fn validate_event(&self, event: &Event) -> Result<(), (String, String)> {
        let ts = (self.clock)();
        match event.kind {
            5 | 13194 | 23194..=23197 => {
                event.verify(ts, &self.limits)
                    .map_err(|e| (event.id.clone(), e.to_string()))
            }
            _ => {
                log_warn!("event rejected: kind={} reason=kind not allowed", event.kind);
                Err((event.id.clone(), "blocked: event kind not allowed".into()))
            }
        }
    }

    /// Route a pre-verified event. Does NOT send OK — the caller is responsible
    /// for sending OK immediately upon successful validation (per NIP-01).
    pub async fn route_verified_event(&mut self, connection_id: u32, event: Event) -> Vec<EngineResponse> {
        if event.kind == 13194 {
            log_info!("info cached: pk={}", short(&event.pubkey, 8));
            self.process_info_event(event.clone()).await;
        } else if event.kind == 5 {
            self.process_deletion_event(&event).await;
        }
        self.route_event(connection_id, event).await
    }

    async fn handle_event(&mut self, connection_id: u32, event: Event) -> Vec<EngineResponse> {
        match event.kind {
            5 | 13194 | 23194..=23197 => {
                let ts = (self.clock)();

                if let Err(e) = event.verify(ts, &self.limits) {
                    return vec![EngineResponse::send(connection_id, RelayMessage::Ok(event.id, false, e.to_string()))];
                }
                
                if event.kind == 13194 {
                    self.process_info_event(event.clone()).await;
                    self.process_event(connection_id, event).await
                } else if event.kind == 5 {
                    self.process_deletion_event(&event).await;
                    self.process_event(connection_id, event).await
                } else {
                    self.process_event(connection_id, event).await
                }
            }
            _ => {
                let ts = (self.clock)();

                let message = if let Err(e) = event.verify(ts, &self.limits) {
                    RelayMessage::Ok(event.id, false, e.to_string())
                } else {
                    RelayMessage::Ok(event.id, false, "blocked: event kind not allowed".into())
                };
                
                vec![EngineResponse::send(connection_id, message)]
            }
        }
    }

    pub async fn handle_req(&mut self, id: u32, sub_id: String, filters: Vec<Filter>) -> Vec<EngineResponse> {
        if filters.iter().any(|f| !f.is_valid()) {
            let message = RelayMessage::Closed(sub_id.clone(), "filter too broad".to_string());
            return vec![EngineResponse::send(id, message)];
        }

        self.process_req(id, sub_id, filters).await
    }

    pub async fn handle_req_internal(&mut self, id: u32, sub_id: String, filters: Vec<Filter>) -> Vec<EngineResponse> {
        self.process_req(id, sub_id, filters).await
    }

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
        let e_tag_ids: Vec<String> = event.tags.iter()
            .filter_map(|t| t.event_id().map(|s| s.to_string()))
            .collect();

        if !e_tag_ids.is_empty() {
            for event_id in &e_tag_ids {
                if let Some(info_pk) = self.registry.find_info_pubkey_by_id(event_id) {
                    if info_pk == *author {
                        log_info!("info deleted by e-tag: pk={} event_id={}", short(&info_pk, 8), short(event_id, 8));
                        self.registry.delete_info(&info_pk).await;
                    }
                }
            }
        } else {
            let k_tags: Vec<u64> = event.tags.iter()
                .filter_map(|t| t.kind_value())
                .collect();

            if k_tags.is_empty() || k_tags.contains(&13194) {
                log_info!("info deleted for author: pk={}", short(author, 8));
                self.registry.delete_info(author).await;
            }
        }
    }

    async fn process_event(&mut self, connection_id: u32, event: Event) -> Vec<EngineResponse> {
        let mut responses = Vec::new();
        
        responses.push(EngineResponse::send(connection_id, RelayMessage::Ok(event.id.clone(), true, "".into())));

        let registry_responses = self.registry.match_event(&event).await;
        for resp in registry_responses {
            match resp {
                RegistryResponse::Send { recipient_id, sub_id } => {
                    responses.push(EngineResponse::send(recipient_id, RelayMessage::Event(sub_id, event.clone())));
                }
                RegistryResponse::WakeUp(recipient_id) => {
                    responses.push(EngineResponse::wake_up(recipient_id));
                }
            }
        }

        responses
    }

    /// Route event to subscribers without sending OK (caller already sent it).
    async fn route_event(&mut self, _connection_id: u32, event: Event) -> Vec<EngineResponse> {
        let mut responses = Vec::new();

        let registry_responses = self.registry.match_event(&event).await;
        log_debug!("event kind={} pk={} → {} subscribers", event.kind, short(&event.pubkey, 8), registry_responses.len());
        for resp in registry_responses {
            match resp {
                RegistryResponse::Send { recipient_id, sub_id } => {
                    responses.push(EngineResponse::send(recipient_id, RelayMessage::Event(sub_id, event.clone())));
                }
                RegistryResponse::WakeUp(recipient_id) => {
                    responses.push(EngineResponse::wake_up(recipient_id));
                }
            }
        }

        responses
    }

    async fn process_req(&mut self, id: u32, sub_id: String, filters: Vec<Filter>) -> Vec<EngineResponse> {
        let mut responses = Vec::new();
        if let Err(e) = self.registry.subscribe(id, sub_id.clone(), filters.clone()).await {
            log_warn!("sub rejected: conn={} sub={}: {}", id, sub_id, e);
            return vec![EngineResponse::send(id, RelayMessage::Closed(sub_id, e.to_string()))];
        }

        let global_limit = filters.iter().filter_map(|f| f.limit).min();

        for filters_set in filters.iter() {
            for pk in filters_set.pubkeys() {
                if let Some(info_event) = self.registry.get_info(&pk).await {
                    if filters.iter().any(|f| f.matches(&info_event)) {
                        log_debug!("info hit: pk={} sub={}", short(&pk, 8), sub_id);
                        responses.push(EngineResponse::send(id, RelayMessage::Event(sub_id.clone(), info_event.clone())));
                    }
                } else {
                    log_debug!("info miss: pk={} sub={}", short(&pk, 8), sub_id);
                }
            }
        }

        if let Some(limit) = global_limit {
            let event_count = responses.iter().filter(|r| matches!(r, EngineResponse::Send { message: RelayMessage::Event(..), .. })).count();
            if event_count >= limit as usize {
                responses.push(EngineResponse::send(id, RelayMessage::Eose(sub_id)));
                return responses;
            }
        }

        responses.push(EngineResponse::send(id, RelayMessage::Eose(sub_id)));
        responses
    }

    pub async fn process_close(&mut self, id: u32, sub_id: String) -> Vec<EngineResponse> {
        log_debug!("close conn={} sub={}", id, sub_id);
        let _ = self.registry.unsubscribe(id, sub_id).await;
        Vec::new()
    }

    pub async fn on_disconnect(&mut self, id: u32) -> Vec<EngineResponse> {
        self.registry.on_disconnect(id).await;
        Vec::new()
    }

    pub async fn on_terminate(&mut self, id: u32) -> Vec<EngineResponse> {
        log_debug!("terminate conn={}", id);
        self.registry.on_terminate(id).await;
        Vec::new()
    }

    pub async fn get_wallet_info(&mut self, pubkey: &str) -> Option<super::WalletInfo> {
        self.registry.get_info(pubkey).await.map(|event| super::nip_47::parse_wallet_info(&event))
    }

    pub async fn add_connection(&mut self, id: u32, subscriptions: HashMap<String, Vec<Filter>>) {
        for (sub_id, filters) in subscriptions {
            let _ = self.registry.subscribe(id, sub_id, filters).await;
        }
    }

    pub async fn load(&mut self, conn_id: u32) -> bool {
        self.registry.load(conn_id).await
    }

    pub async fn load_by_pubkey(&mut self, pubkey: &str) -> Vec<u32> {
        self.registry.load_by_pubkey(pubkey).await
    }
}

#[cfg(test)]
#[path = "engine_test.rs"]
mod engine_test;
