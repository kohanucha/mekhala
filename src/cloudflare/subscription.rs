use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use worker::*;
use wasm_bindgen::JsValue;
use crate::model::{ConnectionState, Filter, Event};
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
        let mut conns = self.connections.borrow_mut();

        if let Some((_, conn_state)) = conns.iter_mut().find(|(w, _)| ws_eq(w, ws)) {
            conn_state.subscriptions.insert(sub_id.clone(), filters.clone());
            Self::update_wallet_index(&mut self.active_wallets.borrow_mut(), &{
                let mut m = std::collections::HashMap::new();
                m.insert(sub_id, filters);
                m
            }, true);
        }

        drop(conns);
        self.rebuild_index();
        let _ = self.update_ws_tags(state, ws);

        Ok(())
    }

    pub fn unsubscribe(&self, state: &State, ws: &WebSocket, sub_id: Option<String>) -> Result<()> {
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

    pub fn broadcast(&self, event: &Event) -> Result<()> {
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
            for filters in conn_state.subscriptions.values() {
                for filter in filters {
                    for pk in filter.pubkeys() {
                        unique.insert(pk);
                    }
                }
            }
            let tags: Vec<String> = unique.into_iter().take(10).collect();
            state.set_tags(&ws, tags);
            ws.serialize_attachment(conn_state)?;
        }
        Ok(())
    }
}

fn ws_eq(a: &WebSocket, b: &WebSocket) -> bool {
    let a_js: &JsValue = a.as_ref();
    let b_js: &JsValue = b.as_ref();
    a_js == b_js
}