use std::collections::{HashMap, HashSet};
use super::{Filter, Event};
use async_trait::async_trait;

#[async_trait(?Send)]
pub trait Storage {
    async fn put(&self, key: &str, value: serde_json::Value);
    async fn get(&self, key: &str) -> Option<serde_json::Value>;
    async fn delete(&self, key: &str);
}

pub struct SavedState {
    pub json: serde_json::Value,
    pub pubkeys: HashSet<String>,
}

#[async_trait(?Send)]
pub trait WalletRegistry {
    async fn subscribe(&mut self, conn_id: u32, sub_id: String, filters: Vec<Filter>);
    async fn unsubscribe(&mut self, conn_id: u32, sub_id: String);
    async fn disconnect(&mut self, conn_id: u32);
    fn get_subscriptions(&self, conn_id: u32) -> HashMap<String, Vec<Filter>>;
    fn cache_info(&mut self, event: Event);
    fn get_info(&self, pubkey: &str) -> Option<Event>;
    fn match_event<'a>(&'a self, event: &'a Event) -> Box<dyn Iterator<Item = (String, Vec<u32>)> + 'a>;
    fn get_connection_id(&self, pubkey: &str) -> Option<u32>;
    async fn save(&self, conn_id: u32) -> Option<SavedState>;
    async fn restore(&mut self, conn_id: u32, data: serde_json::Value);
    async fn load(&mut self, conn_id: u32) -> bool;
    async fn load_by_pubkey(&mut self, pubkey: &str) -> Option<u32>;
}

pub struct InMemoryWalletRegistry {
    subscription_index: HashMap<(String, Vec<Filter>), Vec<u32>>,
    pk_index: HashMap<String, (HashSet<(String, Vec<Filter>)>, Option<Event>)>,
    reverse_index: HashMap<u32, HashMap<String, Vec<Filter>>>,
}

impl InMemoryWalletRegistry {
    pub fn new() -> Self {
        Self {
            subscription_index: HashMap::new(),
            pk_index: HashMap::new(),
            reverse_index: HashMap::new(),
        }
    }

    fn remove_subscription(&mut self, conn_id: u32, sub_id: String) {
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
}

#[async_trait(?Send)]
impl WalletRegistry for InMemoryWalletRegistry {
    async fn subscribe(&mut self, conn_id: u32, sub_id: String, filters: Vec<Filter>) {
        self.remove_subscription(conn_id, sub_id.clone());

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

    async fn unsubscribe(&mut self, conn_id: u32, sub_id: String) {
        self.remove_subscription(conn_id, sub_id);
    }

    async fn disconnect(&mut self, conn_id: u32) {
        let sub_ids: Vec<String> = self.reverse_index.get(&conn_id)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();

        for sub_id in sub_ids {
            self.remove_subscription(conn_id, sub_id);
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
        let mut target_pks = HashSet::new();
        target_pks.insert(event.pubkey.clone());
        for tag in &event.tags {
            if tag.len() >= 2 && tag[0].as_str() == Some("p") {
                if let Some(pk) = tag[1].as_str() {
                    target_pks.insert(pk.to_string());
                }
            }
        }

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

    async fn save(&self, conn_id: u32) -> Option<SavedState> {
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

    async fn restore(&mut self, conn_id: u32, data: serde_json::Value) {
        if let Some(subs_val) = data.get("subscriptions") {
            match serde_json::from_value::<HashMap<String, Vec<Filter>>>(subs_val.clone()) {
                Ok(subs) => {
                    for (sub_id, filters) in subs {
                        self.subscribe(conn_id, sub_id, filters).await;
                    }
                }
                Err(_) => {}
            }
        }
        if let Some(info_val) = data.get("info_event") {
            if let Ok(event) = serde_json::from_value::<Event>(info_val.clone()) {
                self.cache_info(event);
            }
        }
    }

    async fn load(&mut self, _conn_id: u32) -> bool {
        false
    }

    async fn load_by_pubkey(&mut self, pubkey: &str) -> Option<u32> {
        self.get_connection_id(pubkey)
    }
}

pub struct PersistentWalletRegistry<S: Storage> {
    inner: InMemoryWalletRegistry,
    storage: S,
}

impl<S: Storage> PersistentWalletRegistry<S> {
    pub fn new(storage: S) -> Self {
        Self {
            inner: InMemoryWalletRegistry::new(),
            storage,
        }
    }

    async fn sync(&self, conn_id: u32) {
        if let Some(state) = self.inner.save(conn_id).await {
            self.storage.put(&format!("conn:{}", conn_id), state.json).await;
            for pk in state.pubkeys {
                self.storage.put(&format!("pk:{}", pk), serde_json::json!(conn_id)).await;
            }
        } else {
            self.storage.delete(&format!("conn:{}", conn_id)).await;
            // Note: cleaning up pk index is complex, but for personal relay we can let it be.
            // As long as "conn:id" is gone, load(id) will return false.
        }
    }
}

#[async_trait(?Send)]
impl<S: Storage> WalletRegistry for PersistentWalletRegistry<S> {
    async fn subscribe(&mut self, conn_id: u32, sub_id: String, filters: Vec<Filter>) {
        self.inner.subscribe(conn_id, sub_id, filters).await;
        self.sync(conn_id).await;
    }

    async fn unsubscribe(&mut self, conn_id: u32, sub_id: String) {
        self.inner.unsubscribe(conn_id, sub_id).await;
        self.sync(conn_id).await;
    }

    async fn disconnect(&mut self, conn_id: u32) {
        self.inner.disconnect(conn_id).await;
    }

    fn get_subscriptions(&self, conn_id: u32) -> HashMap<String, Vec<Filter>> {
        self.inner.get_subscriptions(conn_id)
    }

    fn cache_info(&mut self, event: Event) {
        self.inner.cache_info(event);
        // Note: we don't sync here because we don't know which connection to sync.
        // The Engine will call sync/save if needed, or subsequent mutations will trigger it.
    }

    fn get_info(&self, pubkey: &str) -> Option<Event> {
        self.inner.get_info(pubkey)
    }

    fn match_event<'a>(&'a self, event: &'a Event) -> Box<dyn Iterator<Item = (String, Vec<u32>)> + 'a> {
        self.inner.match_event(event)
    }

    fn get_connection_id(&self, pubkey: &str) -> Option<u32> {
        self.inner.get_connection_id(pubkey)
    }

    async fn save(&self, conn_id: u32) -> Option<SavedState> {
        self.inner.save(conn_id).await
    }

    async fn restore(&mut self, conn_id: u32, data: serde_json::Value) {
        self.inner.restore(conn_id, data).await;
    }

    async fn load(&mut self, conn_id: u32) -> bool {
        if !self.inner.get_subscriptions(conn_id).is_empty() {
            return true;
        }
        let key = format!("conn:{}", conn_id);
        if let Some(data) = self.storage.get(&key).await {
            self.inner.restore(conn_id, data).await;
            return true;
        }
        false
    }

    async fn load_by_pubkey(&mut self, pubkey: &str) -> Option<u32> {
        if let Some(id) = self.inner.get_connection_id(pubkey) {
            return Some(id);
        }
        let key = format!("pk:{}", pubkey);
        if let Some(val) = self.storage.get(&key).await {
            if let Some(id) = val.as_u64() {
                let id = id as u32;
                if self.load(id).await {
                    return Some(id);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockStorage {
        data: std::sync::Arc<std::sync::Mutex<HashMap<String, serde_json::Value>>>,
    }

    #[async_trait(?Send)]
    impl Storage for MockStorage {
        async fn put(&self, key: &str, value: serde_json::Value) {
            self.data.lock().unwrap().insert(key.to_string(), value);
        }
        async fn get(&self, key: &str) -> Option<serde_json::Value> {
            self.data.lock().unwrap().get(key).cloned()
        }
        async fn delete(&self, key: &str) {
            self.data.lock().unwrap().remove(key);
        }
    }

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
    fn test_registry_matching_grouped() {
        futures::executor::block_on(async {
            let mut registry = InMemoryWalletRegistry::new();

            registry.subscribe(1, "sub1".into(), vec![Filter {
                authors: Some(vec!["alice".into()]),
                ..Default::default()
            }]).await;
            registry.subscribe(2, "sub1".into(), vec![Filter {
                authors: Some(vec!["alice".into()]),
                ..Default::default()
            }]).await;

            let event_alice = make_event("alice", 1, vec![]);
            let matches: Vec<_> = registry.match_event(&event_alice).collect();
            assert_eq!(matches.len(), 1);
            assert_eq!(matches[0].0, "sub1");
            assert_eq!(matches[0].1.len(), 1);
            assert!(matches[0].1.contains(&2));
            assert!(!matches[0].1.contains(&1));
        });
    }

    #[test]
    fn test_info_event_storage() {
        let mut registry = InMemoryWalletRegistry::new();
        let event = make_event("alice", 13194, vec![]);
        registry.cache_info(event.clone());

        let stored = registry.get_info("alice");
        assert!(stored.is_some());
        assert_eq!(stored.unwrap().id, event.id);
    }

    #[test]
    fn test_persistent_registry_sync() {
        futures::executor::block_on(async {
            let storage = MockStorage { data: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())) };
            let mut registry = PersistentWalletRegistry::new(storage);

            registry.subscribe(1, "sub1".into(), vec![Filter {
                authors: Some(vec!["alice".into()]),
                ..Default::default()
            }]).await;

            let data = registry.storage.data.lock().unwrap();
            assert!(data.contains_key("conn:1"));
            assert!(data.contains_key("pk:alice"));
        });
    }
}
