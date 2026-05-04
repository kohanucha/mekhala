use std::collections::{HashMap, HashSet};
use super::state::ConnectionState;
use super::{Filter, Event, RelayMessage};
use crate::util::engine::{Engine, GenericTransport};
use crate::util::now;
use serde_json::Value;

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

    fn handle_nostr_event(&mut self, transport: &dyn GenericTransport, id: u32, event: Event) {
        if let Err(e) = event.verify(now()) {
            transport.send(id, &RelayMessage::Ok(event.id, false, e.to_string()).to_json());
            return;
        }

        transport.send(id, &RelayMessage::Ok(event.id.clone(), true, "".into()).to_json());

        if event.kind == 13194 {
            if let Some(conn_state) = self.connections.get_mut(&id) {
                conn_state.info_event = Some(event.clone());
            }
            let tags = self.compute_tags(id);
            if let Some(conn_state) = self.connections.get(&id) {
                if let Ok(snapshot) = serde_json::to_vec(conn_state) {
                    transport.persist(id, snapshot);
                }
                transport.set_tags(id, tags);
            }
        }

        let mut seen = HashSet::new();
        let mut check_pk = |pk: &str| {
            if let Some(subs) = self.index.get(pk) {
                for (target_id, sub_id) in subs {
                    if seen.insert((*target_id, sub_id.clone())) {
                        if let Some(conn_state) = self.connections.get(target_id) {
                            if let Some(filters) = conn_state.subscriptions.get(sub_id) {
                                if filters.iter().any(|f| f.matches(&event)) {
                                    transport.send(*target_id, &RelayMessage::Event(sub_id.clone(), event.clone()).to_json());
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

    fn handle_nostr_req(&mut self, transport: &dyn GenericTransport, id: u32, sub_id: String, filters: Vec<Filter>) {
        if filters.iter().any(|f| !f.is_valid()) {
            transport.send(id, &RelayMessage::Closed(sub_id, "filter too broad".to_string()).to_json());
            return;
        }

        let mut messages = Vec::new();
        if let Some(conn_state) = self.connections.get_mut(&id) {
            conn_state.subscriptions.insert(sub_id.clone(), filters.clone());
            
            for other_state in self.connections.values() {
                if let Some(info_event) = &other_state.info_event {
                    if filters.iter().any(|f| f.matches(info_event)) {
                        messages.push(RelayMessage::Event(sub_id.clone(), info_event.clone()).to_json());
                    }
                }
            }
        }

        for msg in messages {
            transport.send(id, &msg);
        }

        transport.send(id, &RelayMessage::Eose(sub_id).to_json());

        self.rebuild_index();
        
        let tags = self.compute_tags(id);
        if let Some(conn_state) = self.connections.get(&id) {
            if let Ok(snapshot) = serde_json::to_vec(conn_state) {
                transport.persist(id, snapshot);
            }
            transport.set_tags(id, tags);
        }
    }

    fn handle_nostr_close(&mut self, transport: &dyn GenericTransport, id: u32, sub_id: String) {
        if let Some(conn_state) = self.connections.get_mut(&id) {
            conn_state.subscriptions.remove(&sub_id);
        }

        self.rebuild_index();

        let tags = self.compute_tags(id);
        if let Some(conn_state) = self.connections.get(&id) {
            if let Ok(snapshot) = serde_json::to_vec(conn_state) {
                transport.persist(id, snapshot);
            }
            transport.set_tags(id, tags);
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
}

impl Engine for NostrEngine {
    fn on_connect(&mut self, transport: &dyn GenericTransport, id: u32, state: Option<Vec<u8>>) {
        if let Some(blob) = state {
            if let Ok(conn_state) = serde_json::from_slice::<ConnectionState>(&blob) {
                self.add_connection(id, conn_state);
            }
        } else {
            let mut conn_state = ConnectionState::default();
            conn_state.id = id;
            self.add_connection(id, conn_state.clone());
            if let Ok(snapshot) = serde_json::to_vec(&conn_state) {
                transport.persist(id, snapshot);
            }
        }
    }

    fn on_message(&mut self, transport: &dyn GenericTransport, id: u32, message: &str) {
        let arr: Vec<serde_json::Value> = match serde_json::from_str(message) {
            Ok(a) => a,
            Err(e) => {
                transport.send(id, &RelayMessage::Notice(format!("parse failed: {}", e)).to_json());
                return;
            }
        };

        if arr.is_empty() { return; }

        match arr[0].as_str() {
            Some("EVENT") if arr.len() >= 2 => {
                if let Ok(event) = serde_json::from_value::<Event>(arr[1].clone()) {
                    self.handle_nostr_event(transport, id, event);
                }
            }
            Some("REQ") if arr.len() >= 3 => {
                let sub_id = arr[1].as_str().unwrap_or("").to_string();
                let mut filters = Vec::new();
                for value in &arr[2..] {
                    if let Ok(filter) = serde_json::from_value::<Filter>(value.clone()) {
                        filters.push(filter);
                    }
                }
                self.handle_nostr_req(transport, id, sub_id, filters);
            }
            Some("CLOSE") if arr.len() >= 2 => {
                let sub_id = arr[1].as_str().unwrap_or("").to_string();
                self.handle_nostr_close(transport, id, sub_id);
            }
            _ => {}
        }
    }

    fn on_disconnect(&mut self, _transport: &dyn GenericTransport, id: u32) {
        self.remove_connection(id);
    }

    fn get_info(&self, path: &str) -> Option<serde_json::Value> {
        if path.starts_with("/check/") {
            let pubkey = path.strip_prefix("/check/").unwrap_or("");
            let info = self.get_wallet_info(pubkey);
            let is_online = info.get("online").and_then(|v| v.as_bool()).unwrap_or(false);
            return Some(serde_json::json!(if is_online { "OK" } else { "OFFLINE" }));
        }

        if path.starts_with("/info/") {
            let pubkey = path.strip_prefix("/info/").unwrap_or("");
            return Some(self.get_wallet_info(pubkey));
        }

        None
    }
}
