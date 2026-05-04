use std::collections::{HashMap, HashSet};
use super::state::ConnectionState;
use super::{Filter, Event, RelayMessage};
use serde_json::Value;

pub trait NostrTransport {
    fn send(&self, id: u32, message: RelayMessage);
    fn persist(&self, id: u32, state: &ConnectionState);
    fn set_tags(&self, id: u32, tags: Vec<String>);
}

pub struct NostrEngine {
    connections: HashMap<u32, ConnectionState>,
    index: HashMap<String, Vec<(u32, String)>>,
}

impl NostrEngine {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
            index: HashMap::new(),
        }
    }

    pub fn add_connection(&mut self, id: u32, state: ConnectionState) {
        self.connections.insert(id, state);
        self.rebuild_index();
    }

    pub fn remove_connection(&mut self, id: u32) {
        self.connections.remove(&id);
        self.rebuild_index();
    }

    pub fn subscribe(&mut self, transport: &impl NostrTransport, id: u32, sub_id: String, filters: Vec<Filter>) {
        let mut messages = Vec::new();

        if let Some(conn_state) = self.connections.get_mut(&id) {
            conn_state.subscriptions.insert(sub_id.clone(), filters.clone());
            
            for other_state in self.connections.values() {
                if let Some(info_event) = &other_state.info_event {
                    if filters.iter().any(|f| f.matches(info_event)) {
                        messages.push(RelayMessage::Event(sub_id.clone(), info_event.clone()));
                    }
                }
            }
        }

        for msg in messages {
            transport.send(id, msg);
        }

        self.rebuild_index();
        
        if let Some(conn_state) = self.connections.get(&id) {
            transport.persist(id, conn_state);
            transport.set_tags(id, self.compute_tags(id));
        }
    }

    pub fn unsubscribe(&mut self, transport: &impl NostrTransport, id: u32, sub_id: Option<String>) {
        if let Some(conn_state) = self.connections.get_mut(&id) {
            if let Some(sid) = sub_id {
                conn_state.subscriptions.remove(&sid);
            } else {
                conn_state.subscriptions.clear();
            }
        }

        self.rebuild_index();

        if let Some(conn_state) = self.connections.get(&id) {
            transport.persist(id, conn_state);
            transport.set_tags(id, self.compute_tags(id));
        }
    }

    pub fn save_info_event(&mut self, transport: &impl NostrTransport, id: u32, event: Event) {
        if let Some(conn_state) = self.connections.get_mut(&id) {
            conn_state.info_event = Some(event);
        }

        if let Some(conn_state) = self.connections.get(&id) {
            transport.persist(id, conn_state);
            transport.set_tags(id, self.compute_tags(id));
        }
    }

    pub fn handle_event(&self, transport: &impl NostrTransport, event: &Event) {
        let mut seen = HashSet::new();

        let mut check_pk = |pk: &str| {
            if let Some(subs) = self.index.get(pk) {
                for (id, sub_id) in subs {
                    if seen.insert((*id, sub_id.clone())) {
                        if let Some(conn_state) = self.connections.get(id) {
                            if let Some(filters) = conn_state.subscriptions.get(sub_id) {
                                if filters.iter().any(|f| f.matches(event)) {
                                    transport.send(*id, RelayMessage::Event(sub_id.clone(), event.clone()));
                                }
                            }
                        }
                    }
                }
            }
        };

        check_pk(&event.pubkey);
        for tag in &event.tags {
            if tag.len() >= 2 && tag[0].as_str() == Some("p") {
                if let Some(pk) = tag[1].as_str() {
                    check_pk(pk);
                }
            }
        }
    }

    pub fn get_wallet_info(&self, pubkey: &str) -> Value {
        let mut online = false;
        let mut ready = false;
        let mut encryption = HashSet::new();

        for conn_state in self.connections.values() {
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
                                    match scheme {
                                        "nip44_v2" => { encryption.insert("nip44_v2"); }
                                        "nip04" => { encryption.insert("nip04"); }
                                        _ => {}
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

    fn compute_tags(&self, id: u32) -> Vec<String> {
        let mut tags = Vec::new();
        if let Some(conn_state) = self.connections.get(&id) {
            let mut unique_pks = HashSet::new();
            for filters in conn_state.subscriptions.values() {
                for filter in filters {
                    for pk in filter.pubkeys() {
                        unique_pks.insert(pk);
                    }
                }
            }
            
            tags.extend(unique_pks.into_iter().take(10));

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
                                if scheme == "nip44_v2" { supports_nip44 = true; }
                                else if scheme == "nip04" { supports_nip04 = true; }
                            }
                        }
                    }
                }

                if supports_nip44 { tags.push("cap:nip44".into()); }
                if supports_nip04 || !has_encryption_tag { tags.push("cap:nip04".into()); }
            }
        }
        tags
    }

    fn rebuild_index(&mut self) {
        let mut new_index: HashMap<String, Vec<(u32, String)>> = HashMap::new();
        for (id, state) in &self.connections {
            for (sub_id, filters) in &state.subscriptions {
                for filter in filters {
                    for pk in filter.pubkeys() {
                        new_index.entry(pk)
                            .or_default()
                            .push((*id, sub_id.clone()));
                    }
                }
            }
        }
        self.index = new_index;
    }
}
