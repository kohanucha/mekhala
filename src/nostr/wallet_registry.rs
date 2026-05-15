use std::collections::{HashMap, HashSet};
use async_trait::async_trait;
use serde_json::Value;
use crate::nostr::{Filter, Event};

#[async_trait(?Send)]
pub trait Storage {
    async fn get(&self, key: &str) -> Option<Value>;
    async fn put_batch(&self, entries: HashMap<String, Value>);
    async fn delete_batch(&self, keys: Vec<String>);
}

pub struct SavedState {
    pub json: Value,
    pub pubkeys: HashSet<String>,
}

/// A purely synchronous index for subscriptions and info events.
pub struct WalletIndex {
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

    pub fn get_subscriptions(&self, conn_id: u32) -> HashMap<String, Vec<Filter>> {
        self.reverse_index.get(&conn_id).cloned().unwrap_or_default()
    }

    pub fn cache_info(&mut self, event: Event) {
        let entry = self.pk_index.entry(event.pubkey.clone()).or_insert_with(|| (HashSet::new(), None));
        entry.1 = Some(event);
    }

    pub fn get_info(&self, pubkey: &str) -> Option<Event> {
        self.pk_index.get(pubkey).and_then(|e| e.1.clone())
    }

    pub fn match_event<'a>(&'a self, event: &'a Event) -> Box<dyn Iterator<Item = (String, Vec<u32>)> + 'a> {
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

    fn restore(&mut self, conn_id: u32, data: Value) {
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

pub struct WalletRegistry<S: Storage> {
    pub storage: S,
    pub index: WalletIndex,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RegistryResponse {
    Send { recipient_id: u32, sub_id: String },
    WakeUp(u32),
}

impl<S: Storage> WalletRegistry<S> {
    pub fn new(storage: S) -> Self {
        Self { 
            storage,
            index: WalletIndex::new(),
        }
    }

    pub async fn subscribe(&mut self, conn_id: u32, sub_id: String, filters: Vec<Filter>) -> crate::nostr::Result<()> {
        self.index.subscribe(conn_id, sub_id, filters);
        self.sync(conn_id).await;
        Ok(())
    }

    pub async fn unsubscribe(&mut self, conn_id: u32, sub_id: String) -> crate::nostr::Result<()> {
        self.index.unsubscribe(conn_id, sub_id);
        self.sync(conn_id).await;
        Ok(())
    }

    pub async fn match_event(&mut self, event: &Event) -> Vec<RegistryResponse> {
        let mut responses = Vec::new();
        let target_pks = event.target_pubkeys();

        for pk in target_pks {
            if self.index.get_connection_id(&pk).is_none() {
                if let Some(id) = self.load_by_pubkey(&pk).await {
                    responses.push(RegistryResponse::WakeUp(id));
                }
            }
        }

        for (sub_id, conns) in self.index.match_event(event) {
            for id in conns {
                responses.push(RegistryResponse::Send { recipient_id: id, sub_id: sub_id.clone() });
            }
        }
        responses
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
                    self.storage.delete_batch(vec![key]).await;
                }
            }
        }
        None
    }

    pub async fn on_disconnect(&mut self, id: u32) {
        self.index.disconnect(id);
    }

    pub async fn on_terminate(&mut self, id: u32) {
        self.index.disconnect(id);
        self.storage.delete_batch(vec![format!("conn:{}", id)]).await;
    }

    pub fn cache_info(&mut self, event: Event) {
        self.index.cache_info(event);
    }

    pub fn get_info(&self, pubkey: &str) -> Option<Event> {
        self.index.get_info(pubkey)
    }

    pub fn get_connection_id(&self, pubkey: &str) -> Option<u32> {
        self.index.get_connection_id(pubkey)
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
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    pub struct MockStorage {
        pub data: Arc<Mutex<HashMap<String, Value>>>,
    }

    impl MockStorage {
        pub fn new() -> Self {
            Self {
                data: Arc::new(Mutex::new(HashMap::new())),
            }
        }
    }

    #[async_trait(?Send)]
    impl Storage for MockStorage {
        async fn get(&self, key: &str) -> Option<Value> {
            self.data.lock().unwrap().get(key).cloned()
        }
        async fn put_batch(&self, entries: HashMap<String, Value>) {
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

    #[test]
    fn test_registry_sub_persistence() {
        futures::executor::block_on(async {
            let storage = MockStorage::new();
            let mut registry = WalletRegistry::new(storage);
            
            let filters = vec![Filter {
                authors: Some(vec!["alice".into()]),
                ..Default::default()
            }];
            
            registry.subscribe(1, "sub1".into(), filters).await.unwrap();
            
            // Verify that data was written to storage
            let data = registry.storage.data.lock().unwrap();
            assert!(data.contains_key("conn:1"), "Storage should contain connection state");
            assert!(data.contains_key("pk:alice"), "Storage should contain pubkey index");
        });
    }

    #[test]
    fn test_registry_match_routing() {
        futures::executor::block_on(async {
            let storage = MockStorage::new();
            let mut registry = WalletRegistry::new(storage);
            
            let wallet_pk = "wallet_pk";
            registry.subscribe(1, "sub1".into(), vec![Filter {
                p_tags: Some(vec![wallet_pk.into()]),
                ..Default::default()
            }]).await.unwrap();
            
            let event = Event {
                id: "event1".into(),
                pubkey: "app_pk".into(),
                created_at: 1000,
                kind: 23194,
                tags: vec![vec!["p".into(), wallet_pk.into()]],
                content: "test".into(),
                sig: "sig".into(),
            };
            
            let matches = registry.match_event(&event).await;
            assert_eq!(matches.len(), 1);
            assert_eq!(matches[0], RegistryResponse::Send { recipient_id: 1, sub_id: "sub1".into() });
        });
    }

    #[test]
    fn test_registry_lazy_load() {
        futures::executor::block_on(async {
            let storage = MockStorage::new();
            let wallet_pk = "hibernated_pk";
            let conn_id = 42;
            
            // 1. Seed storage
            let mut entries = HashMap::new();
            entries.insert(format!("pk:{}", wallet_pk), serde_json::json!(conn_id));
            entries.insert(format!("conn:{}", conn_id), serde_json::json!({
                "subscriptions": {
                    "sub1": [{"#p": [wallet_pk]}]
                },
                "info_event": null
            }));
            storage.put_batch(entries).await;
            
            let mut registry = WalletRegistry::new(storage);
            
            // 2. Event targeting the hibernated pubkey
            let event = Event {
                id: "event1".into(),
                pubkey: "app_pk".into(),
                created_at: 1000,
                kind: 23194,
                tags: vec![vec!["p".into(), wallet_pk.into()]],
                content: "test".into(),
                sig: "sig".into(),
            };
            
            let responses = registry.match_event(&event).await;
            
            // 3. Verify WakeUp and Send
            assert!(responses.contains(&RegistryResponse::WakeUp(conn_id)));
            assert!(responses.contains(&RegistryResponse::Send { recipient_id: conn_id, sub_id: "sub1".into() }));
        });
    }
}
