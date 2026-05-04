use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use worker::*;
use crate::cloudflare::ConnectionState;
use crate::nostr::{Filter, Event};
use crate::nostr::RelayMessage;
use crate::cloudflare::{HibernationState, Index};

pub struct SubscriptionManager {
    connections: RefCell<Vec<(WebSocket, ConnectionState)>>,
    active_wallets: RefCell<HashMap<String, usize>>,
    pubkey_index: RefCell<Index>,
}

impl SubscriptionManager {
    pub fn new(state: &State) -> Self {
        let mut connections = Vec::new();
        let mut active_wallets = HashMap::new();

        for ws in state.get_websockets() {
            if let Ok(Some(conn_state)) = ws.deserialize_attachment::<ConnectionState>() {
                Self::update_wallet_index(&mut active_wallets, &conn_state.subscriptions, true);
                connections.push((ws, conn_state));
            }
        }

        let mut index = Index::new();
        index.rebuild(&connections);

        Self {
            connections: RefCell::new(connections),
            active_wallets: RefCell::new(active_wallets),
            pubkey_index: RefCell::new(index),
        }
    }

    pub fn subscribe(&self, state: &State, ws: &WebSocket, sub_id: String, filters: Vec<Filter>) -> Result<()> {
        self.sync(state);
        let mut conns = self.connections.borrow_mut();

        if let Some((_, conn_state)) = conns.iter_mut().find(|(w, _)| ws_eq(w, ws)) {
            conn_state.subscriptions.insert(sub_id.clone(), filters.clone());
            Self::update_wallet_index(&mut self.active_wallets.borrow_mut(), &{
                let mut m = std::collections::HashMap::new();
                m.insert(sub_id.clone(), filters.clone());
                m
            }, true);
        }

        // Send cached info events
        for (_, other_state) in conns.iter() {
            if let Some(info_event) = &other_state.info_event {
                if filters.iter().any(|f| f.matches(info_event)) {
                    let _ = ws.send_with_str(&RelayMessage::Event(sub_id.clone(), info_event.clone()).to_json());
                }
            }
        }

        drop(conns);
        self.rebuild_index();
        let _ = self.update_ws_tags(state, ws);

        Ok(())
    }

    pub fn save_info_event(&self, state: &State, ws: &WebSocket, event: Event) -> Result<()> {
        self.sync(state);
        let mut conns = self.connections.borrow_mut();
        if let Some((_, conn_state)) = conns.iter_mut().find(|(w, _)| ws_eq(w, ws)) {
            conn_state.info_event = Some(event);
            drop(conns);
            let _ = self.update_ws_tags(state, ws);
        }
        Ok(())
    }

    pub fn unsubscribe(&self, state: &State, ws: &WebSocket, sub_id: Option<String>) -> Result<()> {
        self.sync(state);
        let mut conns = self.connections.borrow_mut();

        if let Some((_, conn_state)) = conns.iter_mut().find(|(w, _)| ws_eq(w, ws)) {
            if let Some(id) = sub_id {
                if let Some(old) = conn_state.subscriptions.remove(&id) {
                    let mut m = std::collections::HashMap::new();
                    m.insert(id, old);
                    Self::update_wallet_index(&mut self.active_wallets.borrow_mut(), &m, false);
                }
            } else {
                Self::update_wallet_index(&mut self.active_wallets.borrow_mut(), &conn_state.subscriptions, false);
                conn_state.subscriptions.clear();
            }
        }

        drop(conns);
        self.rebuild_index();
        let _ = self.update_ws_tags(state, ws);

        Ok(())
    }

    pub fn broadcast(&self, state: &State, event: &Event) -> Result<()> {
        self.sync(state);
        let index = self.pubkey_index.borrow();
        let conns = self.connections.borrow();

        let mut candidates: Vec<(usize, String)> = Vec::new();

        let mut add = |pk: &str| {
            for (idx, sid) in index.get_connections(pk) {
                if !candidates.iter().any(|(c, s)| *c == idx && *s == sid) {
                    candidates.push((idx, sid.clone()));
                }
            }
        };

        add(&event.pubkey);

        for tag in &event.tags {
            if tag.len() >= 2 && tag[0].as_str() == Some("p") {
                if let Some(pk) = tag[1].as_str() {
                    add(pk);
                }
            }
        }

        for (conn_idx, sub_id) in candidates {
            if let Some((ws, conn_state)) = conns.get(conn_idx) {
                if let Some(filters) = conn_state.subscriptions.get(&sub_id) {
                    if filters.iter().any(|f| f.matches(event)) {
                        let _ = ws.send_with_str(&RelayMessage::Event(sub_id.clone(), event.clone()).to_json());
                    }
                }
            }
        }

        Ok(())
    }

    pub fn is_wallet_online(&self, pubkey: &str) -> bool {
        self.active_wallets.borrow().get(pubkey).copied().unwrap_or(0) > 0
    }

    pub fn get_wallet_info(&self, pubkey: &str) -> serde_json::Value {
        let conns = self.connections.borrow();

        let mut online = false;
        let mut ready = false;
        let mut encryption = HashSet::new();

        for (_, conn_state) in conns.iter() {
            // Check if any filter in this connection matches the pubkey
            let mut matches_pubkey = false;
            for filters in conn_state.subscriptions.values() {
                for filter in filters {
                    if filter.pubkeys().iter().any(|pk| pk == pubkey) {
                        matches_pubkey = true;
                        break;
                    }
                }
                if matches_pubkey { break; }
            }

            if matches_pubkey {
                online = true;
                if let Some(info_event) = &conn_state.info_event {
                    ready = true;

                    let mut has_encryption_tag = false;
                    for tag in &info_event.tags {
                        if tag.len() >= 2 && tag[0].as_str() == Some("encryption") {
                            has_encryption_tag = true;
                            if let Some(schemes) = tag[1].as_str() {
                                for scheme in schemes.split_whitespace() {
                                    if scheme == "nip44_v2" {
                                        encryption.insert("nip44_v2");
                                    } else if scheme == "nip04" {
                                        encryption.insert("nip04");
                                    }
                                }
                            }
                        }
                    }
                    if !has_encryption_tag {
                        encryption.insert("nip04");
                    }
                }
            }
        }

        serde_json::json!({
            "online": online,
            "ready": ready,
            "encryption": encryption.into_iter().collect::<Vec<_>>()
        })
    }

    fn sync(&self, state: &State) {
        let mut conns = self.connections.borrow_mut();
        let mut active_wallets = self.active_wallets.borrow_mut();
        let active_ws = state.get_websockets();
        let mut changed = false;

        // 1. Remove closed connections
        let mut to_remove = Vec::new();
        for (i, (w, _)) in conns.iter().enumerate() {
            if !active_ws.iter().any(|aw| ws_eq(aw, w)) {
                to_remove.push(i);
            }
        }
        if !to_remove.is_empty() {
            changed = true;
            for i in to_remove.into_iter().rev() {
                let (_, state) = conns.remove(i);
                Self::update_wallet_index(&mut active_wallets, &state.subscriptions, false);
            }
        }

        // 2. Add new connections
        for aw in active_ws {
            if !conns.iter().any(|(w, _)| ws_eq(w, &aw)) {
                changed = true;
                let conn_state = aw.deserialize_attachment::<ConnectionState>().ok().flatten().unwrap_or_default();
                Self::update_wallet_index(&mut active_wallets, &conn_state.subscriptions, true);
                conns.push((aw, conn_state));
            }
        }

        if changed {
            drop(conns);
            drop(active_wallets);
            self.rebuild_index();
        }
    }

    fn rebuild_index(&self) {
        self.pubkey_index.borrow_mut().rebuild(&self.connections.borrow());
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

    fn update_ws_tags(&self, state: &State, ws: &WebSocket) -> Result<()> {
        let conns = self.connections.borrow();
        if let Some((_, conn_state)) = conns.iter().find(|(w, _)| ws_eq(w, ws)) {
            let mut unique: HashSet<String> = HashSet::new();
            
            // Add subscription pubkeys as tags
            for filters in conn_state.subscriptions.values() {
                for filter in filters {
                    for pk in filter.pubkeys() {
                        unique.insert(pk);
                    }
                }
            }
            
            let mut tags: Vec<String> = unique.into_iter().take(10).collect();

            // Add capability tags
            if let Some(info_event) = &conn_state.info_event {
                tags.push("cap:ready".into());

                let mut supports_nip44 = false;
                let mut supports_nip04 = false;
                let mut has_encryption_tag = false;

                for tag in &info_event.tags {
                    if tag.len() >= 2 && tag[0].as_str() == Some("encryption") {
                        has_encryption_tag = true;
                        if let Some(schemes) = tag[1].as_str() {
                            for scheme in schemes.split_whitespace() {
                                if scheme == "nip44_v2" {
                                    supports_nip44 = true;
                                } else if scheme == "nip04" {
                                    supports_nip04 = true;
                                }
                            }
                        }
                    }
                }

                if supports_nip44 {
                    tags.push("cap:nip44".into());
                }
                
                // If tag is present but doesn't mention nip44, or if tag is absent, default to nip04
                if supports_nip04 || !has_encryption_tag {
                    tags.push("cap:nip04".into());
                }
            }

            state.set_tags(&ws, tags);
            ws.serialize_attachment(conn_state)?;
        }
        Ok(())
    }
}

fn ws_eq(a: &WebSocket, b: &WebSocket) -> bool {
    js_sys::Object::is(a.as_ref(), b.as_ref())
}
