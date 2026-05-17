use std::collections::HashMap;
use super::{Filter, Event, RelayMessage, ClientMessage, Limits};
use super::wallet_registry::{WalletRegistry, Storage, RegistryResponse};

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
}

#[cfg(test)]
impl NostrEngine<super::wallet_registry::tests::MockStorage> {
    pub fn new() -> Self {
        Self {
            registry: WalletRegistry::new(super::wallet_registry::tests::MockStorage::new()),
            limits: Limits::default(),
            clock: test_now,
        }
    }
}

impl<S: Storage> NostrEngine<S> {
    pub fn new_with_storage(storage: S, limits: Limits, clock: fn() -> u64) -> Self {
        Self {
            registry: WalletRegistry::new(storage),
            limits,
            clock,
        }
    }

    pub async fn handle(&mut self, connection_id: u32, message: &str) -> Vec<EngineResponse> {
        match ClientMessage::from_json(message) {
            Ok(msg) => self.handle_typed(connection_id, msg).await,
            Err(e) => {
                // NIP-20 Compliance: If we can extract the ID, send an OK false instead of a NOTICE
                if let Some(crate::nostr::nip_01::PartialClientMessage::Event(id)) = crate::nostr::nip_01::PartialClientMessage::from_json(message) {
                    vec![EngineResponse::send(connection_id, RelayMessage::Ok(id, false, format!("parse failed: {}", e)))]
                } else {
                    vec![EngineResponse::send(connection_id, RelayMessage::Notice(format!("parse failed: {}", e)))]
                }
            }
        }
    }

    pub async fn handle_typed(&mut self, connection_id: u32, message: ClientMessage) -> Vec<EngineResponse> {
        match message {
            ClientMessage::Event(event) => self.handle_event(connection_id, event).await,
            ClientMessage::Req(sub_id, filters) => self.handle_req(connection_id, sub_id, filters).await,
            ClientMessage::Close(sub_id) => self.process_close(connection_id, sub_id).await,
        }
    }

    async fn handle_event(&mut self, connection_id: u32, event: Event) -> Vec<EngineResponse> {
        // 1. Kind validation
        match event.kind {
            13194 | 23194..=23197 => {
                // 2. Protocol verification
                let ts = (self.clock)();

                if let Err(e) = event.verify(ts, &self.limits) {
                    return vec![EngineResponse::send(connection_id, RelayMessage::Ok(event.id, false, e.to_string()))];
                }
                
                // 3. Dispatch to engine
                if event.kind == 13194 {
                    self.process_info_event(event.clone()).await;
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

    async fn handle_req(&mut self, id: u32, sub_id: String, filters: Vec<Filter>) -> Vec<EngineResponse> {
        if filters.iter().any(|f| !f.is_valid(&self.limits)) {
            let message = RelayMessage::Closed(sub_id.clone(), "filter too broad".to_string());
            return vec![EngineResponse::send(id, message)];
        }

        self.process_req(id, sub_id, filters).await
    }

    pub async fn on_connect(&mut self, id: u32) -> Vec<EngineResponse> {
        self.add_connection(id, HashMap::new()).await;
        Vec::new()
    }

    async fn process_info_event(&mut self, event: Event) {
        self.registry.cache_info(event).await;
    }

    async fn process_event(&mut self, connection_id: u32, event: Event) -> Vec<EngineResponse> {
        let mut responses = Vec::new();
        
        // Always provide the protocol feedback intent
        responses.push(EngineResponse::send(connection_id, RelayMessage::Ok(event.id.clone(), true, "".into())));

        // Match and route using the deep registry interface
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

    async fn process_req(&mut self, id: u32, sub_id: String, filters: Vec<Filter>) -> Vec<EngineResponse> {
        let mut responses = Vec::new();
        let _ = self.registry.subscribe(id, sub_id.clone(), filters.clone()).await;

        for filters_set in filters.iter() {
            for pk in filters_set.pubkeys() {
                if let Some(info_event) = self.registry.get_info(&pk).await {
                    if filters.iter().any(|f| f.matches(&info_event)) {
                        responses.push(EngineResponse::send(id, RelayMessage::Event(sub_id.clone(), info_event.clone())));
                    }
                }
            }
        }

        responses.push(EngineResponse::send(id, RelayMessage::Eose(sub_id)));
        responses
    }

    pub async fn process_close(&mut self, id: u32, sub_id: String) -> Vec<EngineResponse> {
        let _ = self.registry.unsubscribe(id, sub_id).await;
        Vec::new()
    }

    pub async fn on_disconnect(&mut self, id: u32) -> Vec<EngineResponse> {
        self.registry.on_disconnect(id).await;
        Vec::new()
    }

    pub async fn on_terminate(&mut self, id: u32) -> Vec<EngineResponse> {
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

    pub async fn load_by_pubkey(&mut self, pubkey: &str) -> Option<u32> {
        self.registry.load_by_pubkey(pubkey).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::wallet_registry::tests::MockStorage;
    use super::super::RelayError;

    #[test]
    fn test_engine_req_storage() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();
            engine.on_connect(1).await;

            let req = r#"["REQ", "sub1", {"authors": ["pk1"]}]"#;
            let responses = engine.handle(1, req).await;

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

            let bridge_req = serde_json::json!(["REQ", "sub_bridge", { "#p": [client.my_pubkey] }]).to_string();
            let responses = engine.handle(bridge_id, &bridge_req).await;

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

            let responses = engine.handle(bridge_id, &bridge_event_json).await;

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
            wallet_response_event.tags = vec![super::super::Tag::p(&client.my_pubkey), super::super::Tag::e(&bridge_event.id)];

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
                    "sub1": [{"authors": [pk]}]
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

            let responses = engine.handle(1, &event_json).await;

            let ok_response = responses.iter().find_map(|r| {
                if let EngineResponse::Send { message, .. } = r {
                    if let RelayMessage::Ok(_, success, msg) = message {
                        Some((*success, msg.clone()))
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

            let responses = engine.handle(1, &event_json_recent).await;

            let ok_response = responses.iter().find_map(|r| {
                if let EngineResponse::Send { message, .. } = r {
                    if let RelayMessage::Ok(_, success, msg) = message {
                        Some((*success, msg.clone()))
                    } else { None }
                } else { None }
            });

            assert!(ok_response.is_some(), "should get an OK response");
            let (success, msg) = ok_response.unwrap();
            assert!(msg.contains("invalid"), "event within tolerance should fail for id/sig, not timestamp, got: {}", msg);
            assert!(!success);
        });
    }

    }
