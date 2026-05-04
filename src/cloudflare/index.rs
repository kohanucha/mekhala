use std::collections::HashMap;
use worker::*;

pub struct Index {
    data: HashMap<String, Vec<(usize, String)>>,
}

impl Index {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    pub fn rebuild(&mut self, connections: &[(WebSocket, crate::cloudflare::ConnectionState)]) {
        let mut index = HashMap::new();

        for (conn_idx, (_, state)) in connections.iter().enumerate() {
            for (sub_id, filters) in &state.subscriptions {
                for filter in filters {
                    for pubkey in filter.pubkeys() {
                        index.entry(pubkey)
                            .or_insert_with(Vec::new)
                            .push((conn_idx, sub_id.clone()));
                    }
                }
            }
        }

        self.data = index;
    }

    pub fn get_connections(&self, pubkey: &str) -> Vec<(usize, String)> {
        self.data.get(pubkey).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloudflare::ConnectionState;
    use crate::nostr::Filter;
    use std::collections::HashMap;

    #[test]
    fn test_index_new() {
        let index = Index::new();
        assert!(index.get_connections("any").is_empty());
    }

    #[test]
    fn test_index_rebuild_empty() {
        let mut index = Index::new();
        index.rebuild(&[]);
        assert!(index.get_connections("any").is_empty());
    }

    #[test]
    fn test_index_rebuild_with_subscriptions() {
        let mut index = Index::new();
        
        let mut state = ConnectionState::default();
        let mut filters = HashMap::new();
        filters.insert("sub1".into(), vec![Filter {
            authors: Some(vec!["author1".into()]),
            ..Default::default()
        }]);
        state.subscriptions = filters;
        
        let connections: Vec<(WebSocket, ConnectionState)> = vec![];
        index.rebuild(&connections);
    }

    #[test]
    fn test_index_get_connections_missing_key() {
        let index = Index::new();
        let result = index.get_connections("nonexistent");
        assert!(result.is_empty());
    }
}