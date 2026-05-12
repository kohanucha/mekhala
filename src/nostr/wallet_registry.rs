use std::collections::{HashMap, HashSet};
use super::{Filter, Event};

pub struct SavedState {
    pub json: serde_json::Value,
    pub pubkeys: HashSet<String>,
}

pub trait WalletRegistry {
    fn subscribe(&mut self, conn_id: u32, sub_id: String, filters: Vec<Filter>);
    fn unsubscribe(&mut self, conn_id: u32, sub_id: String);
    fn disconnect(&mut self, conn_id: u32);
    fn get_subscriptions(&self, conn_id: u32) -> HashMap<String, Vec<Filter>>;
    fn cache_info(&mut self, event: Event);
    fn get_info(&self, pubkey: &str) -> Option<Event>;
    fn match_event<'a>(&'a self, event: &'a Event) -> Box<dyn Iterator<Item = (String, Vec<u32>)> + 'a>;
    fn get_connection_id(&self, pubkey: &str) -> Option<u32>;
    fn register_virtual(&mut self, id: u32);
    fn is_virtual(&self, conn_id: u32) -> bool;
    fn save(&self, conn_id: u32) -> Option<SavedState>;
    fn restore(&mut self, conn_id: u32, data: serde_json::Value);
}

pub struct InMemoryWalletRegistry {
    subscription_index: HashMap<(String, Vec<Filter>), Vec<u32>>,
    pk_index: HashMap<String, (HashSet<(String, Vec<Filter>)>, Option<Event>)>,
    reverse_index: HashMap<u32, HashMap<String, Vec<Filter>>>,
    virtual_index: HashSet<u32>,
}

impl InMemoryWalletRegistry {
    pub fn new() -> Self {
        Self {
            subscription_index: HashMap::new(),
            pk_index: HashMap::new(),
            reverse_index: HashMap::new(),
            virtual_index: HashSet::new(),
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

impl WalletRegistry for InMemoryWalletRegistry {
    fn subscribe(&mut self, conn_id: u32, sub_id: String, filters: Vec<Filter>) {
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

    fn unsubscribe(&mut self, conn_id: u32, sub_id: String) {
        self.remove_subscription(conn_id, sub_id);
    }

    fn disconnect(&mut self, conn_id: u32) {
        let sub_ids: Vec<String> = self.reverse_index.get(&conn_id)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();

        for sub_id in sub_ids {
            self.remove_subscription(conn_id, sub_id);
        }

        self.virtual_index.remove(&conn_id);
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

    fn register_virtual(&mut self, id: u32) {
        self.virtual_index.insert(id);
    }

    fn is_virtual(&self, conn_id: u32) -> bool {
        self.virtual_index.contains(&conn_id)
    }

    fn save(&self, conn_id: u32) -> Option<SavedState> {
        let subscriptions = self.get_subscriptions(conn_id);
        if subscriptions.is_empty() && !self.is_virtual(conn_id) {
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
                "virtual_connections": self.virtual_index.iter().copied().collect::<Vec<_>>()
            }),
            pubkeys,
        })
    }

    fn restore(&mut self, conn_id: u32, data: serde_json::Value) {
        if let Some(subs_val) = data.get("subscriptions") {
            match serde_json::from_value::<HashMap<String, Vec<Filter>>>(subs_val.clone()) {
                Ok(subs) => {
                    for (sub_id, filters) in subs {
                        self.subscribe(conn_id, sub_id, filters);
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
    fn test_registry_matching_grouped() {
        let mut registry = InMemoryWalletRegistry::new();

        registry.subscribe(1, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]);
        registry.subscribe(2, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]);

        let event_alice = make_event("alice", 1, vec![]);
        let matches: Vec<_> = registry.match_event(&event_alice).collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "sub1");
        assert_eq!(matches[0].1.len(), 1);
        assert!(matches[0].1.contains(&2));
        assert!(!matches[0].1.contains(&1));
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
    fn test_virtual_connection_lifecycle() {
        let mut registry = InMemoryWalletRegistry::new();

        let id = 0;
        registry.register_virtual(id);
        assert_eq!(id, 0);
        assert!(registry.is_virtual(id));
        assert!(!registry.is_virtual(1));

        registry.disconnect(id);
        assert!(!registry.is_virtual(id));
    }

    #[test]
    fn test_virtual_with_subscription() {
        let mut registry = InMemoryWalletRegistry::new();

        let id = 0;
        registry.register_virtual(id);
        registry.subscribe(id, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]);

        assert!(registry.is_virtual(id));
        let stored = registry.get_subscriptions(id);
        assert!(stored.contains_key("sub1"));

        registry.disconnect(id);
        assert!(!registry.is_virtual(id));
    }

    #[test]
    fn test_export_state_virtual() {
        let mut registry = InMemoryWalletRegistry::new();

        let id = 0;
        registry.register_virtual(id);
        registry.subscribe(id, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]);

        let state = registry.save(id);
        assert!(state.is_some());
        let state = state.unwrap();
        assert!(state.json.get("virtual_connections").is_some());
        let virtual_conns = state.json.get("virtual_connections").unwrap().as_array().unwrap();
        assert!(virtual_conns.contains(&serde_json::json!(0)));
        assert!(state.pubkeys.contains("alice"));
    }
}