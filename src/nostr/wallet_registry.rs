use std::collections::{HashMap, HashSet};
use super::{Filter, Event};

pub struct WalletRegistry {
    // Maps pubkey to a set of (connection_id, subscription_id)
    index: HashMap<String, HashSet<(u32, String)>>,
    // Maps (connection_id, subscription_id) to the filters for that subscription
    // This is needed for the removal and for the matching phase
    subscriptions: HashMap<(u32, String), Vec<Filter>>,
}

impl WalletRegistry {
    pub fn new() -> Self {
        Self {
            index: HashMap::new(),
            subscriptions: HashMap::new(),
        }
    }

    pub fn add_subscription(&mut self, conn_id: u32, sub_id: String, filters: Vec<Filter>) {
        let key = (conn_id, sub_id.clone());
        
        // If updating an existing subscription, remove old index entries first
        self.remove_subscription(conn_id, sub_id.clone());

        for filter in &filters {
            for pk in filter.pubkeys() {
                self.index.entry(pk)
                    .or_default()
                    .insert(key.clone());
            }
        }
        self.subscriptions.insert(key, filters);
    }

    pub fn remove_subscription(&mut self, conn_id: u32, sub_id: String) {
        let key = (conn_id, sub_id);
        if let Some(filters) = self.subscriptions.remove(&key) {
            for filter in filters {
                for pk in filter.pubkeys() {
                    if let Some(subs) = self.index.get_mut(&pk) {
                        subs.remove(&key);
                        if subs.is_empty() {
                            self.index.remove(&pk);
                        }
                    }
                }
            }
        }
    }

    pub fn remove_connection(&mut self, conn_id: u32) {
        // Collect all sub_ids for this connection
        let subs_to_remove: Vec<String> = self.subscriptions.keys()
            .filter(|(c, _)| *c == conn_id)
            .map(|(_, s)| s.clone())
            .collect();

        for sub_id in subs_to_remove {
            self.remove_subscription(conn_id, sub_id);
        }
    }

    pub fn match_event<'a>(&'a self, event: &'a Event) -> impl Iterator<Item = (u32, String)> + 'a {
        let mut potential_matches = HashSet::new();

        // 1. Direct pubkey match
        if let Some(subs) = self.index.get(&event.pubkey) {
            for sub in subs {
                potential_matches.insert(sub);
            }
        }

        // 2. Tagged p-tag matches
        for tag in &event.tags {
            if tag.len() >= 2 && tag[0].as_str() == Some("p") {
                if let Some(pk) = tag[1].as_str() {
                    if let Some(subs) = self.index.get(pk) {
                        for sub in subs {
                            potential_matches.insert(sub);
                        }
                    }
                }
            }
        }

        // 3. Precision filtering
        potential_matches.into_iter().filter_map(move |key| {
            if let Some(filters) = self.subscriptions.get(key) {
                if filters.iter().any(|f| f.matches(event)) {
                    return Some((key.0, key.1.clone()));
                }
            }
            None
        })
    }

    pub fn get_connection_id(&self, pubkey: &str) -> Option<u32> {
        self.index.get(pubkey).and_then(|subs| subs.iter().next().map(|(id, _)| *id))
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
    fn test_registry_matching() {
        let mut registry = WalletRegistry::new();
        
        // Subscription 1: Interested in Alice
        registry.add_subscription(1, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]);

        // Subscription 2: Interested in Bob (via p-tag)
        registry.add_subscription(2, "sub2".into(), vec![Filter {
            p_tags: Some(vec!["bob".into()]),
            ..Default::default()
        }]);

        // Event from Alice
        let event_alice = make_event("alice", 1, vec![]);
        let matches: Vec<_> = registry.match_event(&event_alice).collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0], (1, "sub1".into()));

        // Event to Bob
        let event_to_bob = make_event("carol", 1, vec![
            vec![serde_json::Value::String("p".into()), serde_json::Value::String("bob".into())]
        ]);
        let matches: Vec<_> = registry.match_event(&event_to_bob).collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0], (2, "sub2".into()));
    }

    #[test]
    fn test_registry_removal() {
        let mut registry = WalletRegistry::new();
        registry.add_subscription(1, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]);

        registry.remove_subscription(1, "sub1".into());
        
        let event = make_event("alice", 1, vec![]);
        let matches: Vec<_> = registry.match_event(&event).collect();
        assert_eq!(matches.len(), 0);
    }
}
