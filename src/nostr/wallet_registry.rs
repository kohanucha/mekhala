use std::collections::{HashMap, HashSet};
use super::{Filter, Event};

pub struct WalletRegistry {
    // Maps (sub_id, filters) to a set of connection IDs
    subscription_index: HashMap<(String, Vec<Filter>), HashSet<u32>>,
    // Maps pubkey to (set of (sub_id, filters) that include this pubkey, optional info event)
    pk_index: HashMap<String, (HashSet<(String, Vec<Filter>)>, Option<Event>)>,
    // Maps connection_id to a map of sub_id to filters
    reverse_index: HashMap<u32, HashMap<String, Vec<Filter>>>,
}

impl WalletRegistry {
    pub fn new() -> Self {
        Self {
            subscription_index: HashMap::new(),
            pk_index: HashMap::new(),
            reverse_index: HashMap::new(),
        }
    }

    pub fn add_subscription(&mut self, conn_id: u32, sub_id: String, filters: Vec<Filter>) {
        // If updating an existing subscription, remove old index entries first
        self.remove_subscription(conn_id, sub_id.clone());

        let sub_key = (sub_id.clone(), filters.clone());

        // Update pk_index
        for filter in &filters {
            for pk in filter.pubkeys() {
                let entry = self.pk_index.entry(pk).or_insert_with(|| (HashSet::new(), None));
                entry.0.insert(sub_key.clone());
            }
        }

        // Update subscription_index
        self.subscription_index.entry(sub_key)
            .or_default()
            .insert(conn_id);

        // Update reverse_index
        self.reverse_index.entry(conn_id)
            .or_default()
            .insert(sub_id, filters);
    }

    pub fn remove_subscription(&mut self, conn_id: u32, sub_id: String) {
        if let Some(conn_subs) = self.reverse_index.get_mut(&conn_id) {
            if let Some(filters) = conn_subs.remove(&sub_id) {
                let sub_key = (sub_id, filters);
                
                // Remove from subscription_index
                if let Some(conns) = self.subscription_index.get_mut(&sub_key) {
                    conns.remove(&conn_id);
                    if conns.is_empty() {
                        self.subscription_index.remove(&sub_key);
                        
                        // Only if this (sub_id, filters) is no longer used by ANY connection,
                        // we remove it from pk_index
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

    pub fn remove_connection(&mut self, conn_id: u32) {
        // Collect all sub_ids for this connection
        let sub_ids: Vec<String> = self.reverse_index.get(&conn_id)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();

        for sub_id in sub_ids {
            self.remove_subscription(conn_id, sub_id);
        }

        // Also remove any info events owned by this connection
        // We need a way to track which connection owns which info event.
        // For simplicity, let's assume one info event per connection if we want to clean up,
        // or just let it expire if it's purely pubkey-based.
        // But the prompt says "store in pk_index".
        // If we want to clean up info events, we need a reverse mapping for them too.
    }

    pub fn store_info_event(&mut self, event: Event) {
        let entry = self.pk_index.entry(event.pubkey.clone()).or_insert_with(|| (HashSet::new(), None));
        entry.1 = Some(event);
    }

    pub fn get_info_event(&self, pubkey: &str) -> Option<Event> {
        self.pk_index.get(pubkey).and_then(|e| e.1.clone())
    }

    pub fn match_event<'a>(&'a self, event: &'a Event) -> impl Iterator<Item = (String, Vec<u32>)> + 'a {
        let mut potential_sub_keys = HashSet::new();

        // 1. Direct pubkey match
        if let Some(entry) = self.pk_index.get(&event.pubkey) {
            for sub_key in &entry.0 {
                potential_sub_keys.insert(sub_key);
            }
        }

        // 2. Tagged p-tag matches
        for tag in &event.tags {
            if tag.len() >= 2 && tag[0].as_str() == Some("p") {
                if let Some(pk) = tag[1].as_str() {
                    if let Some(entry) = self.pk_index.get(pk) {
                        for sub_key in &entry.0 {
                            potential_sub_keys.insert(sub_key);
                        }
                    }
                }
            }
        }

        // 3. Precision filtering and connection mapping
        potential_sub_keys.into_iter().filter_map(move |sub_key| {
            if sub_key.1.iter().any(|f| f.matches(event)) {
                if let Some(conns) = self.subscription_index.get(sub_key) {
                    return Some((sub_key.0.clone(), conns.iter().cloned().collect::<Vec<_>>()));
                }
            }
            None
        })
    }

    pub fn get_connection_ids(&self, pubkey: &str) -> Vec<u32> {
        if let Some(entry) = self.pk_index.get(pubkey) {
            let mut ids = HashSet::new();
            for sub_key in &entry.0 {
                if let Some(conns) = self.subscription_index.get(sub_key) {
                    for id in conns {
                        ids.insert(*id);
                    }
                }
            }
            return ids.into_iter().collect();
        }
        Vec::new()
    }

    pub fn export_connection(&self, conn_id: u32) -> Option<serde_json::Value> {
        let subscriptions = self.get_subscriptions(conn_id);
        if subscriptions.is_empty() {
            return None;
        }
        
        // Find if this connection owns any info event (by checking its subscriptions' pubkeys)
        let mut info_event = None;
        for filters in subscriptions.values() {
            for filter in filters {
                for pk in filter.pubkeys() {
                    if let Some(event) = self.get_info_event(&pk) {
                        info_event = Some(event);
                        break;
                    }
                }
                if info_event.is_some() { break; }
            }
            if info_event.is_some() { break; }
        }

        Some(serde_json::json!({
            "subscriptions": subscriptions,
            "info_event": info_event
        }))
    }

    pub fn import_connection(&mut self, conn_id: u32, data: serde_json::Value) {
        if let Some(subs_val) = data.get("subscriptions") {
            if let Ok(subs) = serde_json::from_value::<HashMap<String, Vec<Filter>>>(subs_val.clone()) {
                for (sub_id, filters) in subs {
                    self.add_subscription(conn_id, sub_id, filters);
                }
            }
        }
        if let Some(info_val) = data.get("info_event") {
            if let Ok(event) = serde_json::from_value::<Event>(info_val.clone()) {
                self.store_info_event(event);
            }
        }
    }

    pub fn get_subscriptions(&self, conn_id: u32) -> HashMap<String, Vec<Filter>> {
        self.reverse_index.get(&conn_id).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nostr::Event;

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
        let mut registry = WalletRegistry::new();
        
        // Two connections with identical subscription
        registry.add_subscription(1, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]);
        registry.add_subscription(2, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]);

        // Event from Alice
        let event_alice = make_event("alice", 1, vec![]);
        let matches: Vec<_> = registry.match_event(&event_alice).collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "sub1");
        assert_eq!(matches[0].1.len(), 2);
        assert!(matches[0].1.contains(&1));
        assert!(matches[0].1.contains(&2));
    }

    #[test]
    fn test_info_event_storage() {
        let mut registry = WalletRegistry::new();
        let event = make_event("alice", 13194, vec![]);
        registry.store_info_event(event.clone());
        
        let stored = registry.get_info_event("alice");
        assert!(stored.is_some());
        assert_eq!(stored.unwrap().id, event.id);
    }
}
