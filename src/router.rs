use crate::domain::{Event, Filter};
use crate::relay::{RelayMessage};
use crate::platform::{HibernationState};
use crate::ConnectionState;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use worker::*;
use wasm_bindgen::JsValue;

/// The Router manages active WebSocket connections and their subscriptions.
/// It provides high-performance broadcasting and keeps track of "online" wallets.
pub struct Router {
    /// In-memory cache of ConnectionState for all active WebSockets.
    /// This is the "Speed Layer" that eliminates O(N) deserialization overhead.
    connections: RefCell<Vec<(WebSocket, ConnectionState)>>,

    /// Aggregate index of active wallet pubkeys across all connections.
    /// Used for LNURL bridge "online check".
    active_wallets: RefCell<HashMap<String, usize>>,

    /// High-performance lookup index for pubkey-based filters.
    /// Maps a pubkey to a list of (connection_index, sub_id) pairs.
    pubkey_index: RefCell<HashMap<String, Vec<(usize, String)>>>,
}

impl Router {
    pub fn new(state: &State) -> Self {
        let mut connections = Vec::new();
        let mut active_wallets = HashMap::new();

        for ws in state.get_websockets() {
            if let Ok(Some(conn_state)) = ws.deserialize_attachment::<ConnectionState>() {
                Self::update_wallet_index(&mut active_wallets, &conn_state.subscriptions, true);
                connections.push((ws, conn_state));
            }
        }

        let router = Self {
            connections: RefCell::new(connections),
            active_wallets: RefCell::new(active_wallets),
            pubkey_index: RefCell::new(HashMap::new()),
        };

        router.rebuild_index();
        router
    }

    /// Gets the state for a specific connection, lazily loading it into the cache if missing.
    pub fn get_state(&self, ws: &WebSocket) -> Result<ConnectionState> {
        let mut conns = self.connections.borrow_mut();
        if let Some((_, state)) = conns.iter().find(|(w, _)| {
            let w_js: &JsValue = w.as_ref();
            let target_js: &JsValue = ws.as_ref();
            w_js == target_js
        }) {
            return Ok(state.clone());
        }

        // Lazy load from hibernation state
        let state: ConnectionState = ws.deserialize_attachment()?.unwrap_or_default();
        conns.push((ws.clone(), state.clone()));
        
        // Rebuild index whenever a new connection is cached
        drop(conns);
        self.rebuild_index();
        
        self.connections.borrow().last().map(|(_, s)| s.clone()).ok_or_else(|| Error::from("Failed to cache connection"))
    }

    /// Update subscriptions for a WebSocket.
    pub fn subscribe(&self, state: &State, ws: &WebSocket, sub_id: String, filters: Vec<Filter>, conn_state: &mut ConnectionState) -> Result<()> {
        // 1. Remove old filters for this sub_id from the wallet index
        if let Some(old_filters) = conn_state.subscriptions.get(&sub_id) {
            Self::update_wallet_index(&mut self.active_wallets.borrow_mut(), &{
                let mut m = HashMap::new();
                m.insert(sub_id.clone(), old_filters.clone());
                m
            }, false);
        }

        // 2. Update ConnectionState
        conn_state.subscriptions.insert(sub_id.clone(), filters.clone());
        
        // 3. Update the wallet index with new filters
        Self::update_wallet_index(&mut self.active_wallets.borrow_mut(), &{
            let mut m = HashMap::new();
            m.insert(sub_id, filters);
            m
        }, true);

        // 4. Update memory cache
        self.update_cache(ws, conn_state.clone());

        // 5. Rebuild lookup index
        self.rebuild_index();

        // 6. Update WebSocket tags and attachment (for hibernation)
        self.update_websocket_tags(state, ws, conn_state);
        ws.serialize_attachment(conn_state)?;

        Ok(())
    }

    /// Unsubscribe from a specific sub_id or clear all subscriptions (on close).
    pub fn unsubscribe(&self, state: &State, ws: &WebSocket, sub_id: Option<String>, conn_state: &mut ConnectionState) -> Result<()> {
        let mut wallets = self.active_wallets.borrow_mut();
        let is_specific_sub = sub_id.is_some();
        
        if let Some(id) = sub_id {
            if let Some(old_filters) = conn_state.subscriptions.remove(&id) {
                let mut m = HashMap::new();
                m.insert(id, old_filters);
                Self::update_wallet_index(&mut wallets, &m, false);
            }
            self.update_cache(ws, conn_state.clone());
        } else {
            // Full disconnect
            Self::update_wallet_index(&mut wallets, &conn_state.subscriptions, false);
            conn_state.subscriptions.clear();
            self.remove_from_cache(ws);
        }

        // Rebuild lookup index
        drop(wallets);
        self.rebuild_index();

        if is_specific_sub {
            self.update_websocket_tags(state, ws, conn_state);
        }
        ws.serialize_attachment(conn_state)?;

        Ok(())
    }

    /// Updates the info event for a specific connection in the cache.
    pub fn update_info_event(&self, ws: &WebSocket, event: Event) {
        let mut conns = self.connections.borrow_mut();
        if let Some((_, state)) = conns.iter_mut().find(|(w, _)| {
            let w_js: &JsValue = w.as_ref();
            let target_js: &JsValue = ws.as_ref();
            w_js == target_js
        }) {
            state.info_event = Some(event);
        }
    }

    /// Broadcast an event to matching subscribers using O(1) pubkey lookup.
    pub fn broadcast(&self, event: &Event) -> Result<()> {
        let index = self.pubkey_index.borrow();
        let conns = self.connections.borrow();

        // 1. Identify candidate (connection, sub_id) pairs using the pubkey index
        let mut candidates: Vec<(usize, &String)> = Vec::new();
        
        let mut add_candidates = |pk: &str| {
            if let Some(matches) = index.get(pk) {
                for (conn_idx, sub_id) in matches {
                    if !candidates.iter().any(|(c, s)| *c == *conn_idx && *s == sub_id) {
                        candidates.push((*conn_idx, sub_id));
                    }
                }
            }
        };

        // Check author of the event
        add_candidates(&event.pubkey);
        
        // Check p-tags in the event
        for tag in &event.tags {
            if tag.len() >= 2 && tag[0].as_str() == Some("p") {
                if let Some(p_pubkey) = tag[1].as_str() {
                    add_candidates(p_pubkey);
                }
            }
        }

        // 2. Iterate precise candidates and verify all filter criteria (kinds, since, etc.)
        for (conn_idx, sub_id) in candidates {
            if let Some((ws, conn_state)) = conns.get(conn_idx) {
                if let Some(filters) = conn_state.subscriptions.get(sub_id) {
                    if filters.iter().any(|f| f.matches(event)) {
                        let _ = ws.send_with_str(
                            &RelayMessage::Event(sub_id.clone(), event.clone()).to_json(),
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Retrieves all cached info events matching the provided filters.
    pub fn get_matching_info_events(&self, filters: &[Filter]) -> Vec<Event> {
        let mut matching = Vec::new();
        let conns = self.connections.borrow();

        for (_, state) in conns.iter() {
            if let Some(info) = &state.info_event {
                if filters.iter().any(|f| f.matches(info)) {
                    matching.push(info.clone());
                }
            }
        }
        matching
    }

    pub fn is_wallet_online(&self, pubkey: &str) -> bool {
        self.active_wallets.borrow().get(pubkey).copied().unwrap_or(0) > 0
    }

    /// Rebuilds the high-performance pubkey lookup index.
    fn rebuild_index(&self) {
        let mut index = HashMap::new();
        let conns = self.connections.borrow();

        for (conn_idx, (_, state)) in conns.iter().enumerate() {
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
        *self.pubkey_index.borrow_mut() = index;
    }

    fn update_cache(&self, ws: &WebSocket, conn_state: ConnectionState) {
        let mut conns = self.connections.borrow_mut();
        if let Some(pos) = conns.iter().position(|(w, _)| {
            let w_js: &JsValue = w.as_ref();
            let target_js: &JsValue = ws.as_ref();
            w_js == target_js
        }) {
            conns[pos].1 = conn_state;
        } else {
            conns.push((ws.clone(), conn_state));
        }
    }

    fn remove_from_cache(&self, ws: &WebSocket) {
        let mut conns = self.connections.borrow_mut();
        if let Some(pos) = conns.iter().position(|(w, _)| {
            let w_js: &JsValue = w.as_ref();
            let target_js: &JsValue = ws.as_ref();
            w_js == target_js
        }) {
            conns.swap_remove(pos);
        }
    }

    fn update_wallet_index(wallets: &mut HashMap<String, usize>, subscriptions: &HashMap<String, Vec<Filter>>, increment: bool) {
        for filters in subscriptions.values() {
            for filter in filters {
                for pubkey in filter.pubkeys() {
                    if increment {
                        *wallets.entry(pubkey).or_insert(0) += 1;
                    } else if let Some(count) = wallets.get_mut(&pubkey) {
                        *count = count.saturating_sub(1);
                    }
                }
            }
        }
        if !increment {
            wallets.retain(|_, v| *v > 0);
        }
    }

    fn update_websocket_tags(&self, state: &State, ws: &WebSocket, conn_state: &ConnectionState) {
        let mut unique_pubkeys: HashSet<String> = HashSet::new();
        for filters in conn_state.subscriptions.values() {
            for filter in filters {
                for pubkey in filter.pubkeys() {
                    unique_pubkeys.insert(pubkey);
                }
            }
        }
        let tags: Vec<String> = unique_pubkeys.into_iter().take(10).collect();
        state.set_tags(ws, tags);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_wallet_index() {
        let mut wallets = HashMap::new();
        
        // 1. Add subscription with 1 pubkey
        let mut subs = HashMap::new();
        subs.insert("sub1".into(), vec![Filter { p_tags: Some(vec!["pk1".into()]), ..Default::default() }]);
        Router::update_wallet_index(&mut wallets, &subs, true);
        assert_eq!(wallets.get("pk1"), Some(&1));

        // 2. Add another subscription for SAME pubkey (different sub_id)
        let mut subs2 = HashMap::new();
        subs2.insert("sub2".into(), vec![Filter { p_tags: Some(vec!["pk1".into()]), ..Default::default() }]);
        Router::update_wallet_index(&mut wallets, &subs2, true);
        assert_eq!(wallets.get("pk1"), Some(&2));

        // 3. Remove one subscription
        Router::update_wallet_index(&mut wallets, &subs, false);
        assert_eq!(wallets.get("pk1"), Some(&1));

        // 4. Remove last subscription
        Router::update_wallet_index(&mut wallets, &subs2, false);
        assert_eq!(wallets.get("pk1"), None);
    }

    #[test]
    fn test_router_wallet_online() {
        let router = Router {
            connections: RefCell::new(Vec::new()),
            active_wallets: RefCell::new(HashMap::new()),
            pubkey_index: RefCell::new(HashMap::new()),
        };
        
        assert!(!router.is_wallet_online("pk1"));

        router.active_wallets.borrow_mut().insert("pk1".into(), 1);
        assert!(router.is_wallet_online("pk1"));

        router.active_wallets.borrow_mut().insert("pk1".into(), 0);
        assert!(!router.is_wallet_online("pk1"));
    }
}
