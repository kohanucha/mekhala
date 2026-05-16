use std::collections::{HashMap, HashSet};
use super::{Filter, Event, RelayMessage, ClientMessage, Limits};
use super::wallet_registry::{WalletRegistry, Storage, RegistryResponse};

#[cfg(test)]
use crate::util::now;

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
    pub registry: WalletRegistry<S>,
    limits: Limits,
}

#[cfg(test)]
impl NostrEngine<super::wallet_registry::tests::MockStorage> {
    pub fn new() -> Self {
        Self {
            registry: WalletRegistry::new(super::wallet_registry::tests::MockStorage::new()),
            limits: Limits::default(),
        }
    }
}

impl<S: Storage> NostrEngine<S> {
    pub fn new_with_storage(storage: S, limits: Limits) -> Self {
        Self {
            registry: WalletRegistry::new(storage),
            limits,
        }
    }

    pub async fn handle(&mut self, connection_id: u32, message: &str) -> Vec<EngineResponse> {
        match ClientMessage::from_json(message) {
            Ok(msg) => self.handle_typed(connection_id, msg).await,
            Err(e) => {
                vec![EngineResponse::send(connection_id, RelayMessage::Notice(format!("parse failed: {}", e)))]
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
                #[cfg(not(test))]
                let ts = crate::util::now();
                #[cfg(test)]
                let ts = now();

                if let Err(e) = event.verify(ts, &self.limits) {
                    return vec![EngineResponse::send(connection_id, RelayMessage::Ok(event.id, false, e.to_string()))];
                }
                
                // 3. Dispatch to engine
                if event.kind == 13194 {
                    self.process_info_event(event).await;
                    Vec::new()
                } else {
                    self.process_event(connection_id, event).await
                }
            }
            _ => {
                #[cfg(not(test))]
                let ts = crate::util::now();
                #[cfg(test)]
                let ts = now();

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

    pub async fn process_info_event(&mut self, event: Event) {
        self.registry.cache_info(event);
    }

    pub async fn process_event(&mut self, connection_id: u32, event: Event) -> Vec<EngineResponse> {
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

    pub async fn process_req(&mut self, id: u32, sub_id: String, filters: Vec<Filter>) -> Vec<EngineResponse> {
        let mut responses = Vec::new();
        let _ = self.registry.subscribe(id, sub_id.clone(), filters.clone()).await;

        for filters_set in filters.iter() {
            for pk in filters_set.pubkeys() {
                if let Some(info_event) = self.registry.get_info(&pk) {
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

    pub fn get_wallet_info(&self, pubkey: &str) -> super::WalletInfo {
        let mut online = false;
        let mut ready = false;
        let mut encryption = HashSet::new();

        if let Some(info_event) = self.registry.get_info(pubkey) {
            online = true;
            ready = true;
            let mut has_encryption_tag = false;
            for tag in &info_event.tags {
                if tag.len() >= 2 && tag[0].as_str() == Some("encryption") {
                    has_encryption_tag = true;
                    if let Some(schemes) = tag[1].as_str() {
                        for scheme in schemes.split_whitespace() {
                            match scheme {
                                "nip44_v2" => { encryption.insert("nip44_v2".to_string()); }
                                "nip04" => { encryption.insert("nip04".to_string()); }
                                _ => {}
                            }
                        }
                    }
                }
            }
            if !has_encryption_tag {
                encryption.insert("nip04".to_string());
            }
        } else if self.registry.get_connection_id(pubkey).is_some() {
            online = true;
        }

        super::WalletInfo {
            online,
            ready,
            encryption_algorithms: encryption.into_iter().collect::<Vec<_>>()
        }
    }

    pub fn error_message(&self, msg: &str) -> String {
        RelayMessage::Notice(msg.to_string()).to_json()
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

    fn make_event(pubkey: &str, kind: u64, tags: Vec<Vec<serde_json::Value>>) -> Event {
        Event {
            id: "id".into(),
            pubkey: pubkey.into(),
            kind,
            tags,
            content: "".into(),
            sig: "".into(),
            created_at: 1000,
        }
    }

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

            assert!(engine.registry.index.get_subscriptions(1).contains_key("sub1"));
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

            engine.registry.index.cache_info(event);

            assert!(engine.registry.index.get_info("pk1").is_some());
        });
    }

    #[test]
    fn test_engine_get_wallet_info() {
        let engine = NostrEngine::new();
        let info = engine.get_wallet_info("pk1");
        assert_eq!(info.online, false);
    }

    #[test]
    fn test_bridge_signaling() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();
            engine.on_connect(1).await;
            let wallet_pk = "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f";
            let _ = engine.registry.subscribe(1, "sub1".into(), vec![Filter {
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
                "created_at": now(),
                "kind": 23194,
                "tags": [["p", "bridge"], ["e", "event1"]],
                "content": "",
                "sig": "dummy_sig"
            })).unwrap();
            
            let mut wallet_response_event = wallet_response_event;
            wallet_response_event.tags = vec![vec![serde_json::json!("p"), serde_json::json!(client.my_pubkey)], vec![serde_json::json!("e"), serde_json::json!(bridge_event.id)]];

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
            let _ = engine.registry.subscribe(id, "sub1".into(), vec![Filter {
                authors: Some(vec!["alice".into()]),
                ..Default::default()
            }]).await;

            let event = Event {
                id: "event1".into(),
                pubkey: "alice".into(),
                created_at: now(),
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
            assert!(engine.registry.index.get_subscriptions(id).is_empty());
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
            
            let mut engine = NostrEngine::new_with_storage(storage, Limits::default());

            // 2. Handle an event that targets the hibernated pubkey
            let event = Event {
                id: "event1".into(),
                pubkey: pk.into(),
                created_at: now(),
                kind: 23194,
                tags: vec![vec!["p".into(), pk.into()]],
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
    fn test_index_matching_grouped() {
        let mut engine = NostrEngine::new();

        futures::executor::block_on(async {
            let _ = engine.registry.subscribe(1, "sub1".into(), vec![Filter {
                authors: Some(vec!["alice".into()]),
                ..Default::default()
            }]).await;
            let _ = engine.registry.subscribe(2, "sub1".into(), vec![Filter {
                authors: Some(vec!["alice".into()]),
                ..Default::default()
            }]).await;

            let event_alice = make_event("alice", 1, vec![]);
            let matches: Vec<_> = engine.registry.index.match_event(&event_alice).collect();
            assert_eq!(matches.len(), 1);
            assert_eq!(matches[0].0, "sub1");
            assert_eq!(matches[0].1.len(), 1);
            assert!(matches[0].1.contains(&2));
            assert!(!matches[0].1.contains(&1));
        });
    }

    #[test]
    fn test_info_event_caching() {
        let mut engine = NostrEngine::new();
        let event = make_event("alice", 13194, vec![]);
        engine.registry.index.cache_info(event.clone());

        let stored = engine.registry.index.get_info("alice");
        assert!(stored.is_some());
        assert_eq!(stored.unwrap().id, event.id);
    }

    #[test]
    fn test_registry_sync() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();

            let _ = engine.registry.subscribe(1, "sub1".into(), vec![Filter {
                authors: Some(vec!["alice".into()]),
                ..Default::default()
            }]).await;

            let data = engine.registry.storage.data.lock().unwrap();
            assert!(data.contains_key("conn:1"));
            assert!(data.contains_key("pk:alice"));
        });
    }

    #[test]
    fn test_registry_terminate() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();

            let _ = engine.registry.subscribe(1, "sub1".into(), vec![Filter {
                authors: Some(vec!["alice".into()]),
                ..Default::default()
            }]).await;

            engine.on_terminate(1).await;

            let data = engine.registry.storage.data.lock().unwrap();
            assert!(!data.contains_key("conn:1"));
            // pk:alice is still there (lazy deletion)
            assert!(data.contains_key("pk:alice"));
            assert!(engine.registry.index.get_subscriptions(1).is_empty());
        });
    }

    #[test]
    fn test_registry_lazy_deletion() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();

            // 1. Manually seed a stale pk: pointer
            let mut entries = HashMap::new();
            entries.insert("pk:stale".to_string(), serde_json::json!(99));
            engine.registry.storage.put_batch(entries).await;

            // 2. Attempt to load by pubkey (should fail load(99) and delete pk:stale)
            let result = engine.load_by_pubkey("stale").await;
            assert!(result.is_none());

            let data = engine.registry.storage.data.lock().unwrap();
            assert!(!data.contains_key("pk:stale"));
        });
    }
}
