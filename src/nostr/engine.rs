use std::cell::RefCell;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use k256::schnorr::VerifyingKey;
use lru::LruCache;
use super::{Filter, Event, RelayMessage, ClientMessage, Limits};
use super::wallet_registry::{WalletRegistry, Storage, RegistryResponse};
use crate::util::short;
use crate::{log_info, log_debug, log_warn};

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static TEST_TIME: Cell<u64> = Cell::new(1700000000);
}

#[cfg(test)]
fn test_now() -> u64 {
    TEST_TIME.with(|t| t.get())
}

#[cfg(test)]
fn set_test_time(t: u64) {
    TEST_TIME.with(|c| c.set(t));
}

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
    pubkey_vk_cache: RefCell<LruCache<String, VerifyingKey>>,
}

#[cfg(test)]
impl NostrEngine<super::wallet_registry::tests::MockStorage> {
    pub fn new() -> Self {
        Self {
            registry: WalletRegistry::new(super::wallet_registry::tests::MockStorage::new(), Limits::default()),
            limits: Limits::default(),
            clock: test_now,
            pubkey_vk_cache: RefCell::new(LruCache::new(NonZeroUsize::new(1000).expect("valid"))),
        }
    }
}

impl<S: Storage> NostrEngine<S> {
    pub fn new_with_storage(storage: S, limits: Limits, clock: fn() -> u64) -> Self {
        Self {
            registry: WalletRegistry::new(storage, limits),
            limits,
            clock,
            pubkey_vk_cache: RefCell::new(LruCache::new(NonZeroUsize::new(1000).expect("valid"))),
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
                let key = &event.pubkey;
                let cached = self.pubkey_vk_cache.borrow_mut().get(key).cloned();
                match cached {
                    Some(vk) => event.verify_with_key(ts, &self.limits, &vk)
                        .map_err(|e| (event.id.clone(), e.to_string())),
                    None => {
                        match event.verify(ts, &self.limits) {
                            Ok(()) => {
                                // Cache the key for next time
                                if let Ok(pk_bytes) = hex::decode(key) {
                                    if let Ok(vk) = VerifyingKey::from_bytes(&pk_bytes) {
                                        self.pubkey_vk_cache.borrow_mut().put(key.clone(), vk);
                                    }
                                }
                                Ok(())
                            }
                            Err(e) => Err((event.id.clone(), e.to_string())),
                        }
                    }
                }
            }
            _ => {
                log_warn!("event rejected: kind={} reason=kind not allowed", event.kind);
                Err((event.id.clone(), "blocked: event kind not allowed".into()))
            }
        }
    }

    /// Route a pre-verified event. Does NOT send OK — the caller is responsible
    /// for sending OK immediately after successful validation (per NIP-01).
    pub async fn route_verified_event(&mut self, connection_id: u32, event: Arc<Event>) -> Vec<EngineResponse> {
        if event.kind == 13194 {
            log_info!("info cached: pk={}", short(&event.pubkey, 8));
            self.process_info_event(Event::clone(&event)).await;
        } else if event.kind == 5 {
            self.process_deletion_event(event.as_ref()).await;
        }
        self.route_event(connection_id, event).await
    }

    async fn handle_event(&mut self, connection_id: u32, event: Event) -> Vec<EngineResponse> {
        match self.validate_event(&event) {
            Ok(()) => {
                let event = Arc::new(event);
                let ok = EngineResponse::send(connection_id, RelayMessage::Ok(event.id.clone(), true, "".into()));
                let mut rest = self.route_verified_event(connection_id, event).await;
                rest.insert(0, ok);
                rest
            }
            Err((id, reason)) => {
                vec![EngineResponse::send(connection_id, RelayMessage::Ok(id, false, reason))]
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

    #[allow(dead_code)]
    async fn process_event(&mut self, connection_id: u32, event: Event) -> Vec<EngineResponse> {
        let event = Arc::new(event);
        let mut responses = Vec::new();

        responses.push(EngineResponse::send(connection_id, RelayMessage::Ok(event.id.clone(), true, "".into())));

        let registry_responses = self.registry.match_event(event.as_ref()).await;
        for resp in registry_responses {
            match resp {
                RegistryResponse::Send { recipient_id, sub_id } => {
                    responses.push(EngineResponse::send(recipient_id, RelayMessage::Event(sub_id, Arc::clone(&event))));
                }
                RegistryResponse::WakeUp(recipient_id) => {
                    responses.push(EngineResponse::wake_up(recipient_id));
                }
            }
        }

        responses
    }

    /// Route event to subscribers without sending OK (caller already sent it).
    async fn route_event(&mut self, _connection_id: u32, event: Arc<Event>) -> Vec<EngineResponse> {
        let mut responses = Vec::new();

        let registry_responses = self.registry.match_event(event.as_ref()).await;
        log_debug!("event kind={} pk={} → {} subscribers", event.kind, short(&event.pubkey, 8), registry_responses.len());
        for resp in registry_responses {
            match resp {
                RegistryResponse::Send { recipient_id, sub_id } => {
                    responses.push(EngineResponse::send(recipient_id, RelayMessage::Event(sub_id, Arc::clone(&event))));
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
                        responses.push(EngineResponse::send(id, RelayMessage::Event(sub_id.clone(), Arc::new(info_event))));
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

    #[cfg(test)]
    pub fn has_subscription(&self, conn_id: u32, sub_id: &str) -> bool {
        self.registry.has_subscription(conn_id, sub_id)
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
mod tests {
    use super::*;
    use super::super::wallet_registry::tests::MockStorage;
    use super::super::RelayError;
    use super::super::Tag;

    #[test]
    fn test_engine_req_storage() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();
            engine.on_connect(1).await;

            let req = r#"["REQ", "sub1", {"kinds": [23194], "authors": ["pk1"]}]"#;
            let msg = ClientMessage::from_json(req).unwrap();
            let responses = engine.handle_typed(1, msg).await;

            assert!(responses.iter().any(|r| {
                if let EngineResponse::Send { message, .. } = r {
                    matches!(message, RelayMessage::Eose(_))
                } else {
                    false
                }
            }));

            assert!(engine.has_subscription(1, "sub1"));
        });
    }

    #[test]
    fn test_engine_info_event_routing() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();
            engine.on_connect(1).await;

            let event = Event {
                id: "id1".into(),
                pubkey: "pk1".into(),
                created_at: 1000,
                kind: 13194,
                tags: vec![],
                content: "".into(),
                sig: "sig1".into(),
            };

            engine.process_info_event(event).await;

            assert!(engine.get_wallet_info("pk1").await.is_some());
            });
            }

            #[test]
            fn test_get_wallet_info_none() {
                let mut engine = NostrEngine::new();
                futures::executor::block_on(async {
                    assert!(engine.get_wallet_info("pk1").await.is_none());
                });
            }
    #[test]
    fn test_engine_get_wallet_info_with_encryption_tag() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();
            let event = Event {
                id: "id1".into(),
                pubkey: "pk1".into(),
                created_at: 1000,
                kind: 13194,
                tags: vec![super::super::Tag::encryption("nip44_v2 nip04")],
                content: "".into(),
                sig: "sig1".into(),
            };
            engine.process_info_event(event).await;

            let info = engine.get_wallet_info("pk1").await.unwrap();
            assert!(info.encryption_algorithms.contains(&super::super::nip_47::EncryptionMethod::Nip44));
            assert!(info.encryption_algorithms.contains(&super::super::nip_47::EncryptionMethod::Nip04));
        });
    }

    #[test]
    fn test_engine_get_wallet_info_default_nip04() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();
            let event = Event {
                id: "id1".into(),
                pubkey: "pk1".into(),
                created_at: 1000,
                kind: 13194,
                tags: vec![],
                content: "".into(),
                sig: "sig1".into(),
            };
            engine.process_info_event(event).await;

            let info = engine.get_wallet_info("pk1").await.unwrap();
            assert_eq!(info.encryption_algorithms, vec![super::super::nip_47::EncryptionMethod::Nip04]);
        });
    }

    #[test]
    fn test_bridge_signaling() {
        futures::executor::block_on(async {
            set_test_time(crate::util::now());
            let mut engine = NostrEngine::new();
            engine.on_connect(1).await;
            let wallet_pk = "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f";
            let _ = engine.process_req(1, "sub1".into(), vec![Filter {
                kinds: Some(vec![23194]),
                p_tags: Some(vec![wallet_pk.into()]),
                ..Default::default()
            }]).await;

            let bridge_id = 100;
            let bridge_sk = "0202020202020202020202020202020202020202020202020202020202020202";

            // Create a valid signed event for the bridge
            let uri = crate::nostr::nip_47::NwcUri {
                wallet_pubkey: wallet_pk.to_string(),
                secret: bridge_sk.to_string(),
            };
            let client = crate::nostr::nip_47::NwcClient::new(uri).unwrap();

            let bridge_req = serde_json::json!(["REQ", "sub_bridge", {"kinds": [23194], "#p": [client.my_pubkey]}]).to_string();
            let msg = ClientMessage::from_json(&bridge_req).unwrap();
            let responses = engine.handle_typed(bridge_id, msg).await;

            // REQ should return EOSE Send
            assert!(responses.iter().any(|r| matches!(r, EngineResponse::Send { recipient_id: 100, message: RelayMessage::Eose(_) })));

            // Create a valid signed event for the bridge
            let uri = crate::nostr::nip_47::NwcUri {
                wallet_pubkey: wallet_pk.to_string(),
                secret: bridge_sk.to_string(),
            };
            let client = crate::nostr::nip_47::NwcClient::new(uri).unwrap();
            let (bridge_event, _) = client.create_request_event(crate::nostr::nip_47::NwcMethod::MakeInvoice, serde_json::json!({}), vec![]).unwrap();

            let bridge_event_json = serde_json::json!([
                "EVENT",
                bridge_event
            ]).to_string();

            let msg = ClientMessage::from_json(&bridge_event_json).unwrap();
            let responses = engine.handle_typed(bridge_id, msg).await;

            // Event should be routed to connection 1 as Send
            assert!(responses.iter().any(|r| matches!(r, EngineResponse::Send { recipient_id: 1, .. })));
            // EVENT should return OK Send
            assert!(responses.iter().any(|r| matches!(r, EngineResponse::Send { recipient_id: 100, message: RelayMessage::Ok(_, true, _) })));

            let wallet_response_event: Event = serde_json::from_value(serde_json::json!({
                "id": "resp1",
                "pubkey": "wallet_pk",
                "created_at": test_now(),
                "kind": 23194,
                "tags": [["p", "bridge"], ["e", "event1"]],
                "content": "",
                "sig": "dummy_sig"
            })).unwrap();
            
            let mut wallet_response_event = wallet_response_event;
            wallet_response_event.tags = vec![super::super::Tag::p(&client.my_pubkey), super::super::Tag::E(bridge_event.id.clone(), vec![])];

            // Wallet response comes from connection 1
            let responses = engine.process_event(1, wallet_response_event).await;

            // Routed EVENT SHOULD go to connection 100 as Send
            assert!(responses.iter().any(|r| {
                if let EngineResponse::Send { recipient_id, message } = r {
                    if let RelayMessage::Event(_, event) = message {
                        *recipient_id == 100 && event.id == "resp1"
                    } else {
                        false
                    }
                } else {
                    false
                }
            }));
        });
    }

    #[test]
    fn test_virtual_connection_lifecycle() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();
            engine.on_connect(1).await;

            let id = 100;
            let _ = engine.process_req(id, "sub1".into(), vec![Filter {
                authors: Some(vec!["alice".into()]),
                kinds: Some(vec![23194]),
                ..Default::default()
            }]).await;

            let event = Event {
                id: "event1".into(),
                pubkey: "alice".into(),
                created_at: test_now(),
                kind: 23194,
                tags: vec![],
                content: "test".into(),
                sig: "sig".into(),
            };

            let responses = engine.process_event(2, event).await;

            assert!(responses.iter().any(|r| {
                if let EngineResponse::Send { recipient_id, message } = r {
                    if let RelayMessage::Event(_, event) = message {
                        *recipient_id == 100 && event.id == "event1"
                    } else {
                        false
                    }
                } else {
                    false
                }
            }));

            engine.on_disconnect(id).await;
            assert!(!engine.has_subscription(id, "sub1"));
        });
    }

    #[test]
    fn test_engine_wakeup_logic() {
        futures::executor::block_on(async {
            // 1. Seed the storage with a "hibernated" connection
            let id = 42;
            let pk = "hibernated_pk";
            let storage = MockStorage::new();
            let mut entries = HashMap::new();
            entries.insert(format!("pk:{}", pk), serde_json::json!(id));
            entries.insert(format!("conn:{}", id), serde_json::json!({
                "subscriptions": {
                    "sub1": [{"kinds": [23194], "authors": [pk]}]
                },
                "info_event": null
            }));
            storage.put_batch(entries).await;
            
            let mut engine = NostrEngine::new_with_storage(storage, Limits::default(), test_now);

            // 2. Handle an event that targets the hibernated pubkey
            let event = Event {
                id: "event1".into(),
                pubkey: pk.into(),
                created_at: test_now(),
                kind: 23194,
                tags: vec![super::super::Tag::p(pk)],
                content: "wake up!".into(),
                sig: "sig".into(),
            };

            let responses = engine.process_event(99, event).await;

            // 3. Verify that a WakeUp response was returned
            assert!(responses.iter().any(|r| matches!(r, EngineResponse::WakeUp { connection_id: 42 })));
            
            // 4. Verify that a Data response was ALSO returned (since matching happens after loading)
            assert!(responses.iter().any(|r| matches!(r, EngineResponse::Send { recipient_id: 42, .. })));
        });
    }

    #[test]
    fn test_event_rejects_future_beyond_tolerance() {
        let now = 1700000000u64;
        let event = Event {
            id: "id1".into(),
            pubkey: "pk1".into(),
            created_at: now + 901,
            kind: 13194,
            tags: vec![],
            content: "".into(),
            sig: "sig1".into(),
        };

        let result = event.verify(now, &Limits::default());
        match result {
            Err(RelayError::TimestampTooFar(_)) => {},
            Err(e) => panic!("expected TimestampTooFar, got {:?}", e),
            Ok(_) => panic!("event 901s in the future should be rejected"),
        }
    }

    #[test]
    fn test_event_accepts_future_within_tolerance() {
        let now = 1700000000u64;
        let event = Event {
            id: "id1".into(),
            pubkey: "pk1".into(),
            created_at: now + 800,
            kind: 13194,
            tags: vec![],
            content: "".into(),
            sig: "sig1".into(),
        };

        let result = event.verify(now, &Limits::default());
        match result {
            Err(RelayError::TimestampTooFar(_)) => panic!("event within 900s future should not fail timestamp check"),
            Err(RelayError::InvalidId) | Err(RelayError::InvalidSignature) => {},
            Err(e) => panic!("expected id/sig error, got {:?}", e),
            Ok(_) => panic!("expected error (id mismatch)"),
        }
    }

    #[test]
    fn test_event_rejects_past_beyond_tolerance() {
        let now = 1700000000u64;
        let event = Event {
            id: "id1".into(),
            pubkey: "pk1".into(),
            created_at: now - 31_536_001,
            kind: 13194,
            tags: vec![],
            content: "".into(),
            sig: "sig1".into(),
        };

        let result = event.verify(now, &Limits::default());
        match result {
            Err(RelayError::TimestampTooFar(_)) => {},
            Err(e) => panic!("expected TimestampTooFar, got {:?}", e),
            Ok(_) => panic!("event over 1 year old should be rejected"),
        }
    }

    #[test]
    fn test_event_accepts_past_within_tolerance() {
        let now = 1700000000u64;
        let event = Event {
            id: "id1".into(),
            pubkey: "pk1".into(),
            created_at: now - 100000,
            kind: 13194,
            tags: vec![],
            content: "".into(),
            sig: "sig1".into(),
        };

        let result = event.verify(now, &Limits::default());
        match result {
            Err(RelayError::TimestampTooFar(_)) => panic!("event within 1 year should not be rejected for timestamp"),
            Err(RelayError::InvalidId) | Err(RelayError::InvalidSignature) => {},
            Err(e) => panic!("expected id/sig error, got {:?}", e),
            Ok(_) => panic!("expected error (id mismatch)"),
        }
    }

    #[test]
    fn test_engine_uses_clock_for_event_verification() {
        futures::executor::block_on(async {
            let now = 1700000000u64;
            set_test_time(now);
            let mut engine = NostrEngine::new();
            engine.on_connect(1).await;

            let event_json = serde_json::json!(["EVENT", {
                "id": "fake_id",
                "pubkey": "pk1",
                "created_at": now + 901,
                "kind": 23194,
                "tags": [],
                "content": "test",
                "sig": "badsig"
            }]).to_string();

            let msg = ClientMessage::from_json(&event_json).unwrap();
            let responses = engine.handle_typed(1, msg).await;

            let ok_response = responses.iter().find_map(|r| {
                if let EngineResponse::Send { message, .. } = r {
                    if let RelayMessage::Ok(_, success, msg) = message {
                        Some((success, msg.clone()))
                    } else { None }
                } else { None }
            });

            assert!(ok_response.is_some(), "should get an OK response");
            let (success, msg) = ok_response.unwrap();
            assert!(!success, "event too far in future should be rejected");
            assert!(msg.contains("too far"), "expected timestamp rejection, got: {}", msg);

            set_test_time(now + 901);
            let event_json_recent = serde_json::json!(["EVENT", {
                "id": "fake_id2",
                "pubkey": "pk1",
                "created_at": now + 901,
                "kind": 23194,
                "tags": [],
                "content": "test",
                "sig": "badsig"
            }]).to_string();

            let msg = ClientMessage::from_json(&event_json_recent).unwrap();
            let responses = engine.handle_typed(1, msg).await;

            let ok_response = responses.iter().find_map(|r| {
                if let EngineResponse::Send { message, .. } = r {
                    if let RelayMessage::Ok(_, success, msg) = message {
                        Some((success, msg.clone()))
                    } else { None }
                } else { None }
            });

            assert!(ok_response.is_some(), "should get an OK response");
            let (success, msg) = ok_response.unwrap();
            assert!(msg.contains("invalid"), "event within tolerance should fail for id/sig, not timestamp, got: {}", msg);
            assert!(!success);
        });
    }

    #[test]
    fn test_kind_5_deletion_with_e_tag() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();
            engine.on_connect(1).await;

            // Publish info event for alice
            let info_event = Event {
                id: "info1".into(),
                pubkey: "alice".into(),
                created_at: test_now(),
                kind: 13194,
                tags: vec![],
                content: "".into(),
                sig: "sig1".into(),
            };
            engine.process_info_event(info_event.clone()).await;
            assert!(engine.get_wallet_info("alice").await.is_some());

            // Delete it via kind 5 with e-tag referencing the info event
            let deletion_event = Event {
                id: "del1".into(),
                pubkey: "alice".into(),
                created_at: test_now(),
                kind: 5,
                tags: vec![Tag::E("info1".into(), vec![])],
                content: "deleted".into(),
                sig: "sig2".into(),
            };
            engine.process_deletion_event(&deletion_event).await;

            // Info should be gone
            assert!(engine.get_wallet_info("alice").await.is_none());
        });
    }

    #[test]
    fn test_kind_5_deletion_unauthorized() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();
            engine.on_connect(1).await;

            // Publish info event for alice
            let info_event = Event {
                id: "info1".into(),
                pubkey: "alice".into(),
                created_at: test_now(),
                kind: 13194,
                tags: vec![],
                content: "".into(),
                sig: "sig1".into(),
            };
            engine.process_info_event(info_event.clone()).await;
            assert!(engine.get_wallet_info("alice").await.is_some());

            // Try to delete with wrong pubkey (bob)
            let deletion_event = Event {
                id: "del1".into(),
                pubkey: "bob".into(),
                created_at: test_now(),
                kind: 5,
                tags: vec![Tag::E("info1".into(), vec![])],
                content: "deleted".into(),
                sig: "sig2".into(),
            };
            engine.process_deletion_event(&deletion_event).await;

            // Info should still be there (bob can't delete alice's info)
            assert!(engine.get_wallet_info("alice").await.is_some());
        });
    }

    #[test]
    fn test_kind_5_deletion_with_k_tag() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();
            engine.on_connect(1).await;

            let info_event = Event {
                id: "info1".into(),
                pubkey: "alice".into(),
                created_at: test_now(),
                kind: 13194,
                tags: vec![],
                content: "".into(),
                sig: "sig1".into(),
            };
            engine.process_info_event(info_event.clone()).await;
            assert!(engine.get_wallet_info("alice").await.is_some());

            // Delete via k=13194 tag without e-tag
            let deletion_event = Event {
                id: "del1".into(),
                pubkey: "alice".into(),
                created_at: test_now(),
                kind: 5,
                tags: vec![Tag::Other("k".into(), vec![serde_json::json!("13194")])],
                content: "deleted".into(),
                sig: "sig2".into(),
            };
            engine.process_deletion_event(&deletion_event).await;

            assert!(engine.get_wallet_info("alice").await.is_none());
        });
    }

    #[test]
    fn test_kind_5_deletion_no_tags() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();
            engine.on_connect(1).await;

            let info_event = Event {
                id: "info1".into(),
                pubkey: "alice".into(),
                created_at: test_now(),
                kind: 13194,
                tags: vec![],
                content: "".into(),
                sig: "sig1".into(),
            };
            engine.process_info_event(info_event.clone()).await;
            assert!(engine.get_wallet_info("alice").await.is_some());

            // Delete with no e/k tags (delete all events by this author)
            let deletion_event = Event {
                id: "del1".into(),
                pubkey: "alice".into(),
                created_at: test_now(),
                kind: 5,
                tags: vec![],
                content: "deleted".into(),
                sig: "sig2".into(),
            };
            engine.process_deletion_event(&deletion_event).await;

            assert!(engine.get_wallet_info("alice").await.is_none());
        });
    }

    }
