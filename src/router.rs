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
    /// Aggregate index of active wallet pubkeys across all connections.
    /// Used for LNURL bridge "online check".
    active_wallets: RefCell<HashMap<String, usize>>,
}

impl Router {
    pub fn new(state: &State) -> Self {
        let mut active_wallets = HashMap::new();

        for ws in state.get_websockets() {
            if let Ok(Some(conn_state)) = ws.deserialize_attachment::<ConnectionState>() {
                Self::update_wallet_index(&mut active_wallets, &conn_state.subscriptions, true);
            }
        }

        Self {
            active_wallets: RefCell::new(active_wallets),
        }
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

        // 4. Update WebSocket tags and attachment (for hibernation)
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
        } else {
            // Full disconnect
            Self::update_wallet_index(&mut wallets, &conn_state.subscriptions, false);
            conn_state.subscriptions.clear();
        }

        if is_specific_sub {
            self.update_websocket_tags(state, ws, conn_state);
        }
        ws.serialize_attachment(conn_state)?;

        Ok(())
    }

    /// Broadcast an event to all matching subscribers.
    pub fn broadcast(&self, state: &State, event: &Event) -> Result<()> {
        let mut target_websockets: Vec<WebSocket> = Vec::new();

        let mut add_if_unique = |target_ws: WebSocket| {
            if !target_websockets.iter().any(|w| {
                let w_js: &JsValue = w.as_ref();
                let target_js: &JsValue = target_ws.as_ref();
                w_js == target_js
            }) {
                target_websockets.push(target_ws);
            }
        };

        // Fast-path: Use Cloudflare tags (pubkeys)
        if state.is_tags_supported() {
            for target_ws in state.get_tagged_websockets(&event.pubkey) {
                add_if_unique(target_ws);
            }
            for tag in &event.tags {
                if tag.len() >= 2 && tag[0].as_str() == Some("p") {
                    if let Some(p_pubkey) = tag[1].as_str() {
                        for target_ws in state.get_tagged_websockets(p_pubkey) {
                            add_if_unique(target_ws);
                        }
                    }
                }
            }
        }

        // Slow-path fallback: If no tags matched, iterate all.
        // Even if tags matched, we still need to check filters for exact match.
        let check_list: Vec<WebSocket> = if !target_websockets.is_empty() {
             target_websockets
        } else {
             state.get_websockets()
        };

        for target_ws in check_list {
            let other_state: ConnectionState = match target_ws.deserialize_attachment() {
                Ok(Some(s)) => s,
                _ => continue,
            };
            for (sub_id, filters) in &other_state.subscriptions {
                if filters.iter().any(|f| f.matches(event)) {
                    let _ = target_ws.send_with_str(
                        &RelayMessage::Event(sub_id.clone(), event.clone()).to_json(),
                    );
                }
            }
        }

        Ok(())
    }

    pub fn is_wallet_online(&self, pubkey: &str) -> bool {
        self.active_wallets.borrow().get(pubkey).copied().unwrap_or(0) > 0
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
            active_wallets: RefCell::new(HashMap::new()),
        };
        
        assert!(!router.is_wallet_online("pk1"));

        router.active_wallets.borrow_mut().insert("pk1".into(), 1);
        assert!(router.is_wallet_online("pk1"));

        router.active_wallets.borrow_mut().insert("pk1".into(), 0);
        assert!(!router.is_wallet_online("pk1"));
    }
}
