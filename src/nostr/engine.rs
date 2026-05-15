use std::collections::{HashMap, HashSet};
use super::{Filter, Event, RelayMessage, ClientMessage};
use async_trait::async_trait;

#[cfg(test)]
use crate::util::now;

#[derive(Debug, PartialEq, Eq)]
pub enum EngineResponse {
    /// Data intended for a subscriber (e.g., EVENT, EOSE).
    Data { recipient_id: u32, message: RelayMessage },
    /// Protocol feedback for the message sender (e.g., OK, NOTICE, CLOSED).
    Reply { recipient_id: u32, message: RelayMessage },
    /// Internal control signal to wake up a hibernated connection.
    WakeUp { connection_id: u32 },
}

impl EngineResponse {
    pub fn data(recipient_id: u32, message: RelayMessage) -> Self {
        EngineResponse::Data { recipient_id, message }
    }

    pub fn reply(recipient_id: u32, message: RelayMessage) -> Self {
        EngineResponse::Reply { recipient_id, message }
    }

    pub fn wake_up(connection_id: u32) -> Self {
        EngineResponse::WakeUp { connection_id }
    }
}

#[async_trait(?Send)]
pub trait Storage {
    async fn get(&self, key: &str) -> Option<serde_json::Value>;
    async fn put_batch(&self, entries: HashMap<String, serde_json::Value>);
    async fn delete_batch(&self, keys: Vec<String>);
}

pub struct SavedState {
    pub json: serde_json::Value,
    pub pubkeys: HashSet<String>,
}

/// A purely synchronous index for subscriptions and info events.
struct WalletIndex {
    subscription_index: HashMap<(String, Vec<Filter>), Vec<u32>>,
    pk_index: HashMap<String, (HashSet<(String, Vec<Filter>)>, Option<Event>)>,
    reverse_index: HashMap<u32, HashMap<String, Vec<Filter>>>,
}

impl WalletIndex {
    fn new() -> Self {
        Self {
            subscription_index: HashMap::new(),
            pk_index: HashMap::new(),
            reverse_index: HashMap::new(),
        }
    }

    fn subscribe(&mut self, conn_id: u32, sub_id: String, filters: Vec<Filter>) {
        self.unsubscribe(conn_id, sub_id.clone());

        let sub_key = (sub_id.clone(), filters.clone());

        for filter in &filters {
            for pk in filter.pubkeys() {
                let entry = self.pk_index.entry(pk).or_insert_with(|| (HashSet::new(), None));
                entry.0.insert(sub_key.clone());
            }
        }

        let conns = self.subscription_index.entry(sub_key).or_default();
        if !conns.contains(&conn_id) {
            conns.push(conn_id);
        }

        self.reverse_index.entry(conn_id)
            .or_default()
            .insert(sub_id, filters);
    }

    fn unsubscribe(&mut self, conn_id: u32, sub_id: String) {
        if let Some(conn_subs) = self.reverse_index.get_mut(&conn_id) {
            if let Some(filters) = conn_subs.remove(&sub_id) {
                let sub_key = (sub_id, filters);

                if let Some(conns) = self.subscription_index.get_mut(&sub_key) {
                    conns.retain(|&id| id != conn_id);
                    if conns.is_empty() {
                        self.subscription_index.remove(&sub_key);

                        for filter in &sub_key.1 {
                            for pk in filter.pubkeys() {
                                if let Some(entry) = self.pk_index.get_mut(&pk) {
                                    entry.0.remove(&sub_key);
                                    if entry.0.is_empty() && entry.1.is_none() {
                                        self.pk_index.remove(&pk);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if conn_subs.is_empty() {
                self.reverse_index.remove(&conn_id);
            }
        }
    }

    fn disconnect(&mut self, conn_id: u32) {
        let sub_ids: Vec<String> = self.reverse_index.get(&conn_id)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();

        for sub_id in sub_ids {
            self.unsubscribe(conn_id, sub_id);
        }
    }

    fn get_subscriptions(&self, conn_id: u32) -> HashMap<String, Vec<Filter>> {
        self.reverse_index.get(&conn_id).cloned().unwrap_or_default()
    }

    fn cache_info(&mut self, event: Event) {
        let entry = self.pk_index.entry(event.pubkey.clone()).or_insert_with(|| (HashSet::new(), None));
        entry.1 = Some(event);
    }

    fn get_info(&self, pubkey: &str) -> Option<Event> {
        self.pk_index.get(pubkey).and_then(|e| e.1.clone())
    }

    fn match_event<'a>(&'a self, event: &'a Event) -> Box<dyn Iterator<Item = (String, Vec<u32>)> + 'a> {
        let target_pks = event.target_pubkeys();

        let mut sub_to_conns = HashMap::new();
        for pk in target_pks {
            if let Some(entry) = self.pk_index.get(&pk) {
                let global_latest = self.get_connection_id(&pk);

                for sub_key in &entry.0 {
                    if sub_key.1.iter().any(|f| f.matches(event)) {
                        if let Some(conns) = self.subscription_index.get(sub_key) {
                            if let Some(latest_conn) = conns.last() {
                                if global_latest == Some(*latest_conn) {
                                    sub_to_conns.insert(sub_key.0.clone(), vec![*latest_conn]);
                                }
                            }
                        }
                    }
                }
            }
        }
        Box::new(sub_to_conns.into_iter())
    }

    fn get_connection_id(&self, pubkey: &str) -> Option<u32> {
        if let Some(entry) = self.pk_index.get(pubkey) {
            let mut latest = None;
            for sub_key in &entry.0 {
                if let Some(conns) = self.subscription_index.get(sub_key) {
                    if let Some(&id) = conns.last() {
                        match latest {
                            None => latest = Some(id),
                            Some(current) if id > current => latest = Some(id),
                            _ => {}
                        }
                    }
                }
            }
            return latest;
        }
        None
    }

    fn save(&self, conn_id: u32) -> Option<SavedState> {
        let subscriptions = self.get_subscriptions(conn_id);
        if subscriptions.is_empty() {
            return None;
        }

        let mut info_event = None;
        let mut pubkeys = HashSet::new();
        for filters in subscriptions.values() {
            for filter in filters {
                for pk in filter.pubkeys() {
                    pubkeys.insert(pk.clone());
                    if info_event.is_none() {
                        if let Some(event) = self.get_info(&pk) {
                            info_event = Some(event);
                        }
                    }
                }
            }
        }

        Some(SavedState {
            json: serde_json::json!({
                "subscriptions": subscriptions,
                "info_event": info_event,
            }),
            pubkeys,
        })
    }

    fn restore(&mut self, conn_id: u32, data: serde_json::Value) {
        if let Some(subs_val) = data.get("subscriptions") {
            if let Ok(subs) = serde_json::from_value::<HashMap<String, Vec<Filter>>>(subs_val.clone()) {
                for (sub_id, filters) in subs {
                    self.subscribe(conn_id, sub_id, filters);
                }
            }
        }
        if let Some(info_val) = data.get("info_event") {
            if let Ok(event) = serde_json::from_value::<Event>(info_val.clone()) {
                self.cache_info(event);
            }
        }
    }
}

pub struct NostrEngine<S: Storage> {
    index: WalletIndex,
    pub storage: S,
}

#[cfg(test)]
impl NostrEngine<MockStorage> {
    pub fn new() -> Self {
        Self {
            index: WalletIndex::new(),
            storage: MockStorage::new(),
        }
    }
}

impl<S: Storage> NostrEngine<S> {
    pub fn new_with_storage(storage: S) -> Self {
        Self {
            index: WalletIndex::new(),
            storage,
        }
    }

    pub async fn handle(&mut self, connection_id: u32, message: &str) -> Vec<EngineResponse> {
        match ClientMessage::from_json(message) {
            Ok(msg) => self.handle_typed(connection_id, msg).await,
            Err(e) => {
                vec![EngineResponse::reply(connection_id, RelayMessage::Notice(format!("parse failed: {}", e)))]
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

                if let Err(e) = event.verify(ts) {
                    return vec![EngineResponse::reply(connection_id, RelayMessage::Ok(event.id, false, e.to_string()))];
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

                let message = if let Err(e) = event.verify(ts) {
                    RelayMessage::Ok(event.id, false, e.to_string())
                } else {
                    RelayMessage::Ok(event.id, false, "blocked: event kind not allowed".into())
                };
                
                vec![EngineResponse::reply(connection_id, message)]
            }
        }
    }

    async fn handle_req(&mut self, id: u32, sub_id: String, filters: Vec<Filter>) -> Vec<EngineResponse> {
        if filters.iter().any(|f| !f.is_valid()) {
            let message = RelayMessage::Closed(sub_id.clone(), "filter too broad".to_string());
            return vec![EngineResponse::data(id, message)];
        }

        self.process_req(id, sub_id, filters).await
    }

    pub async fn on_connect(&mut self, id: u32) -> Vec<EngineResponse> {
        self.add_connection(id, HashMap::new()).await;
        Vec::new()
    }

    pub async fn process_info_event(&mut self, event: Event) {
        self.index.cache_info(event);
    }

    pub async fn process_event(&mut self, connection_id: u32, event: Event) -> Vec<EngineResponse> {
        let mut responses = Vec::new();
        
        // Always provide the protocol feedback intent
        responses.push(EngineResponse::reply(connection_id, RelayMessage::Ok(event.id.clone(), true, "".into())));

        // 1. Identify target pubkeys for routing
        let target_pks = event.target_pubkeys();

        // 2. For each pubkey, ensure the connection is loaded
        for pk in target_pks {
            // Check if connection is already in memory
            if self.index.get_connection_id(&pk).is_none() {
                // Not in memory, try loading from storage
                if let Some(rid) = self.load_by_pubkey(&pk).await {
                    // Found in storage and now loaded into memory, signal a wake-up
                    responses.push(EngineResponse::wake_up(rid));
                }
            }
        }

        // 3. Match and route to active connections
        let matches: Vec<(String, Vec<u32>)> = self.index.match_event(&event).collect();
        for (client_id, recipient_ids) in matches {
            for rid in recipient_ids {
                responses.push(EngineResponse::data(rid, RelayMessage::Event(client_id.clone(), event.clone())));
            }
        }

        responses
    }

    pub async fn process_req(&mut self, id: u32, sub_id: String, filters: Vec<Filter>) -> Vec<EngineResponse> {
        let mut responses = Vec::new();
        self.subscribe(id, sub_id.clone(), filters.clone()).await;

        for filters_set in filters.iter() {
            for pk in filters_set.pubkeys() {
                if let Some(info_event) = self.index.get_info(&pk) {
                    if filters.iter().any(|f| f.matches(&info_event)) {
                        responses.push(EngineResponse::data(id, RelayMessage::Event(sub_id.clone(), info_event.clone())));
                    }
                }
            }
        }

        responses.push(EngineResponse::data(id, RelayMessage::Eose(sub_id)));
        responses
    }

    pub async fn process_close(&mut self, id: u32, sub_id: String) -> Vec<EngineResponse> {
        self.unsubscribe(id, sub_id).await;
        Vec::new()
    }

    pub async fn on_disconnect(&mut self, id: u32) -> Vec<EngineResponse> {
        self.index.disconnect(id);
        Vec::new()
    }

    pub async fn on_terminate(&mut self, id: u32) -> Vec<EngineResponse> {
        self.index.disconnect(id);
        self.storage.delete_batch(vec![format!("conn:{}", id)]).await;
        Vec::new()
    }

    pub fn get_wallet_info(&self, pubkey: &str) -> super::WalletInfo {
        let mut online = false;
        let mut ready = false;
        let mut encryption = HashSet::new();

        if let Some(info_event) = self.index.get_info(pubkey) {
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
        } else if self.index.get_connection_id(pubkey).is_some() {
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
            self.subscribe(id, sub_id, filters).await;
        }
    }

    async fn sync(&self, conn_id: u32) {
        if let Some(state) = self.index.save(conn_id) {
            let mut entries = HashMap::new();
            entries.insert(format!("conn:{}", conn_id), state.json);
            for pk in state.pubkeys {
                entries.insert(format!("pk:{}", pk), serde_json::json!(conn_id));
            }
            self.storage.put_batch(entries).await;
        } else {
            self.storage.delete_batch(vec![format!("conn:{}", conn_id)]).await;
        }
    }

    pub async fn subscribe(&mut self, conn_id: u32, sub_id: String, filters: Vec<Filter>) {
        self.index.subscribe(conn_id, sub_id, filters);
        self.sync(conn_id).await;
    }

    pub async fn unsubscribe(&mut self, conn_id: u32, sub_id: String) {
        self.index.unsubscribe(conn_id, sub_id);
        self.sync(conn_id).await;
    }

    pub async fn load(&mut self, conn_id: u32) -> bool {
        if !self.index.get_subscriptions(conn_id).is_empty() {
            return true;
        }
        let key = format!("conn:{}", conn_id);
        if let Some(data) = self.storage.get(&key).await {
            self.index.restore(conn_id, data);
            return true;
        }
        false
    }

    pub async fn load_by_pubkey(&mut self, pubkey: &str) -> Option<u32> {
        if let Some(id) = self.index.get_connection_id(pubkey) {
            return Some(id);
        }
        let key = format!("pk:{}", pubkey);
        if let Some(val) = self.storage.get(&key).await {
            if let Some(id) = val.as_u64() {
                let id = id as u32;
                if self.load(id).await {
                    return Some(id);
                } else {
                    // Lazy deletion of stale pointer
                    self.storage.delete_batch(vec![key]).await;
                }
            }
        }
        None
    }
}

#[cfg(test)]
pub struct MockStorage {
    pub data: std::sync::Arc<std::sync::Mutex<HashMap<String, serde_json::Value>>>,
}

#[cfg(test)]
impl MockStorage {
    pub fn new() -> Self {
        Self {
            data: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }
}

#[cfg(test)]
#[async_trait(?Send)]
impl Storage for MockStorage {
    async fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.data.lock().unwrap().get(key).cloned()
    }
    async fn put_batch(&self, entries: HashMap<String, serde_json::Value>) {
        let mut data = self.data.lock().unwrap();
        for (k, v) in entries {
            data.insert(k, v);
        }
    }
    async fn delete_batch(&self, keys: Vec<String>) {
        let mut data = self.data.lock().unwrap();
        for k in keys {
            data.remove(&k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                if let EngineResponse::Data { message, .. } = r {
                    matches!(message, RelayMessage::Eose(_))
                } else {
                    false
                }
            }));

            assert!(engine.index.get_subscriptions(1).contains_key("sub1"));
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

            engine.index.cache_info(event);

            assert!(engine.index.get_info("pk1").is_some());
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
            engine.subscribe(1, "sub1".into(), vec![Filter {
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

            // REQ should return EOSE Data intent
            assert!(responses.iter().any(|r| matches!(r, EngineResponse::Data { recipient_id: 100, message: RelayMessage::Eose(_) })));

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

            // Event should be routed to connection 1 as Data
            assert!(responses.iter().any(|r| matches!(r, EngineResponse::Data { recipient_id: 1, .. })));
            // EVENT should return OK Reply intent
            assert!(responses.iter().any(|r| matches!(r, EngineResponse::Reply { recipient_id: 100, message: RelayMessage::Ok(_, true, _) })));

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

            // Routed EVENT SHOULD go to connection 100 as Data
            assert!(responses.iter().any(|r| {
                if let EngineResponse::Data { recipient_id, message } = r {
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
            engine.subscribe(id, "sub1".into(), vec![Filter {
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
                if let EngineResponse::Data { recipient_id, message } = r {
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
            assert!(engine.index.get_subscriptions(id).is_empty());
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
            
            let mut engine = NostrEngine::new_with_storage(storage);

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
            assert!(responses.iter().any(|r| matches!(r, EngineResponse::Data { recipient_id: 42, .. })));
        });
    }

    #[test]
    fn test_index_matching_grouped() {
        let mut engine = NostrEngine::new();

        futures::executor::block_on(async {
            engine.subscribe(1, "sub1".into(), vec![Filter {
                authors: Some(vec!["alice".into()]),
                ..Default::default()
            }]).await;
            engine.subscribe(2, "sub1".into(), vec![Filter {
                authors: Some(vec!["alice".into()]),
                ..Default::default()
            }]).await;

            let event_alice = make_event("alice", 1, vec![]);
            let matches: Vec<_> = engine.index.match_event(&event_alice).collect();
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
        engine.index.cache_info(event.clone());

        let stored = engine.index.get_info("alice");
        assert!(stored.is_some());
        assert_eq!(stored.unwrap().id, event.id);
    }

    #[test]
    fn test_registry_sync() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();

            engine.subscribe(1, "sub1".into(), vec![Filter {
                authors: Some(vec!["alice".into()]),
                ..Default::default()
            }]).await;

            let data = engine.storage.data.lock().unwrap();
            assert!(data.contains_key("conn:1"));
            assert!(data.contains_key("pk:alice"));
        });
    }

    #[test]
    fn test_registry_terminate() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();

            engine.subscribe(1, "sub1".into(), vec![Filter {
                authors: Some(vec!["alice".into()]),
                ..Default::default()
            }]).await;

            engine.on_terminate(1).await;

            let data = engine.storage.data.lock().unwrap();
            assert!(!data.contains_key("conn:1"));
            // pk:alice is still there (lazy deletion)
            assert!(data.contains_key("pk:alice"));
            assert!(engine.index.get_subscriptions(1).is_empty());
        });
    }

    #[test]
    fn test_registry_lazy_deletion() {
        futures::executor::block_on(async {
            let mut engine = NostrEngine::new();

            // 1. Manually seed a stale pk: pointer
            let mut entries = HashMap::new();
            entries.insert("pk:stale".to_string(), serde_json::json!(99));
            engine.storage.put_batch(entries).await;

            // 2. Attempt to load by pubkey (should fail load(99) and delete pk:stale)
            let result = engine.load_by_pubkey("stale").await;
            assert!(result.is_none());

            let data = engine.storage.data.lock().unwrap();
            assert!(!data.contains_key("pk:stale"));
        });
    }
}
