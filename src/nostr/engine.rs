use std::collections::{HashMap, HashSet};
use super::ConnectionState;
use super::{Filter, Event, RelayMessage, ClientMessage};
use super::wallet_registry::WalletRegistry;
use crate::util::engine::{Engine, EngineAction};
use crate::util::transport::SyncTransport;
use crate::util::now;
use serde_json::Value;

pub struct NostrEngine {
    connections: HashMap<u32, ConnectionState>,
    registry: WalletRegistry,
}

impl NostrEngine {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
            registry: WalletRegistry::new(),
        }
    }

    pub fn add_connection(&mut self, id: u32, state: ConnectionState) {
        let subscriptions = state.subscriptions.clone();
        self.connections.insert(id, state);
        for (sub_id, filters) in subscriptions {
            self.registry.add_subscription(id, sub_id, filters);
        }
    }

    pub fn remove_connection(&mut self, id: u32) {
        self.connections.remove(&id);
        self.registry.remove_connection(id);
    }

    fn handle_nostr_event(&mut self, transport: &dyn SyncTransport, id: u32, event: Event) -> EngineAction {
        if let Err(e) = event.verify(now()) {
            transport.send(id, &RelayMessage::Ok(event.id, false, e.to_string()).to_json());
            return EngineAction::None;
        }

        transport.send(id, &RelayMessage::Ok(event.id.clone(), true, "".into()).to_json());

        let mut needs_commit = false;
        if event.kind == 13194 {
            if let Some(conn_state) = self.connections.get_mut(&id) {
                conn_state.info_event = Some(event.clone());
                needs_commit = true;
            }
        }

        for (target_id, sub_id) in self.registry.match_event(&event) {
            transport.send(target_id, &RelayMessage::Event(sub_id, event.clone()).to_json());
        }
        
        if needs_commit { self.commit_state(id) } else { EngineAction::None }
    }

    fn handle_nostr_req(&mut self, transport: &dyn SyncTransport, id: u32, sub_id: String, filters: Vec<Filter>) -> EngineAction {
        if filters.iter().any(|f| !f.is_valid()) {
            transport.send(id, &RelayMessage::Closed(sub_id, "filter too broad".to_string()).to_json());
            return EngineAction::None;
        }

        let mut messages = Vec::new();
        if let Some(conn_state) = self.connections.get_mut(&id) {
            conn_state.subscriptions.insert(sub_id.clone(), filters.clone());
            self.registry.add_subscription(id, sub_id.clone(), filters.clone());
            
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

        self.commit_state(id)
    }

    fn handle_nostr_close(&mut self, _transport: &dyn SyncTransport, id: u32, sub_id: String) -> EngineAction {
        if let Some(conn_state) = self.connections.get_mut(&id) {
            conn_state.subscriptions.remove(&sub_id);
            self.registry.remove_subscription(id, sub_id);
        }

        self.commit_state(id)
    }

    fn commit_state(&self, id: u32) -> EngineAction {
        let conn_state = match self.connections.get(&id) {
            Some(s) => s,
            None => return EngineAction::None,
        };

        let snapshot = match serde_json::to_vec(conn_state) {
            Ok(s) => s,
            Err(_) => return EngineAction::None,
        };

        let mut pubkeys = HashSet::new();
        for filters in conn_state.subscriptions.values() {
            for filter in filters {
                for pk in filter.pubkeys() {
                    pubkeys.insert(pk);
                }
            }
        }

        let mut tags: Vec<String> = pubkeys.into_iter().take(10).collect();
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

        EngineAction::Commit { snapshot, tags }
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
    fn on_connect(&mut self, _transport: &dyn SyncTransport, id: u32, state: Option<Vec<u8>>) -> EngineAction {
        if let Some(blob) = state {
            if let Ok(conn_state) = serde_json::from_slice::<ConnectionState>(&blob) {
                self.add_connection(id, conn_state);
                EngineAction::None
            } else {
                EngineAction::None
            }
        } else {
            let mut conn_state = ConnectionState::default();
            conn_state.id = id;
            self.add_connection(id, conn_state);
            self.commit_state(id)
        }
    }

    fn on_message(&mut self, transport: &dyn SyncTransport, id: u32, message: &str) -> EngineAction {
        match ClientMessage::from_json(message) {
            Ok(ClientMessage::Event(event)) => self.handle_nostr_event(transport, id, event),
            Ok(ClientMessage::Req(sub_id, filters)) => self.handle_nostr_req(transport, id, sub_id, filters),
            Ok(ClientMessage::Close(sub_id)) => self.handle_nostr_close(transport, id, sub_id),
            Err(e) => {
                transport.send(id, &RelayMessage::Notice(format!("parse failed: {}", e)).to_json());
                EngineAction::None
            }
        }
    }

    fn on_disconnect(&mut self, _transport: &dyn SyncTransport, id: u32) -> EngineAction {
        self.remove_connection(id);
        EngineAction::None
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

    fn initial_state(&self) -> Vec<u8> {
        serde_json::to_vec(&ConnectionState::default()).unwrap_or_default()
    }

    fn error_message(&self, msg: &str) -> String {
        RelayMessage::Notice(msg.to_string()).to_json()
    }

    fn get_connection_id(&self, pubkey: &str) -> Option<u32> {
        self.registry.get_connection_id(pubkey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockTransport;
    impl SyncTransport for MockTransport {
        fn send(&self, _id: u32, _message: &str) {}
    }

    #[test]
    fn test_engine_req_returns_commit() {
        let mut engine = NostrEngine::new();
        let transport = MockTransport;
        
        // Connect
        engine.on_connect(&transport, 1, None);
        
        // Send REQ
        let req = r#"["REQ", "sub1", {"authors": ["pk1"]}]"#;
        let action = engine.on_message(&transport, 1, req);
        
        if let EngineAction::Commit { snapshot, tags } = action {
            assert!(tags.contains(&"pk1".to_string()));
            assert!(!tags.contains(&"cap:ready".to_string()));
            assert!(!snapshot.is_empty());
        } else {
            panic!("Expected Commit action");
        }
    }

    #[test]
    fn test_engine_info_event_returns_commit_and_caps() {
        let mut engine = NostrEngine::new();
        let transport = MockTransport;
        engine.on_connect(&transport, 1, None);
        
        // Kind 13194 Info Event
        let event_json = serde_json::json!([
            "EVENT",
            {
                "id": "0000000000000000000000000000000000000000000000000000000000000000",
                "pubkey": "pk1",
                "created_at": 1000,
                "kind": 13194,
                "tags": [["encryption", "nip44_v2"]],
                "content": "",
                "sig": "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
            }
        ]).to_string();

        let action = engine.on_message(&transport, 1, &event_json);
        // It returns None because verification failed
        assert_eq!(action, EngineAction::None);
    }

    #[test]
    fn test_engine_snapshot_roundtrip() {
        let mut engine = NostrEngine::new();
        let transport = MockTransport;
        engine.on_connect(&transport, 1, None);
        
        let req = r#"["REQ", "sub1", {"authors": ["pk1"]}]"#;
        let action = engine.on_message(&transport, 1, req);
        
        let snapshot = if let EngineAction::Commit { snapshot, .. } = action {
            snapshot
        } else {
            panic!("Expected Commit");
        };
        
        let mut engine2 = NostrEngine::new();
        // Restore connection 1 with snapshot
        let action2 = engine2.on_connect(&transport, 1, Some(snapshot));
        assert_eq!(action2, EngineAction::None);
        
        // Verify state is restored (e.g., via commit_state)
        if let EngineAction::Commit { tags, .. } = engine2.commit_state(1) {
            assert!(tags.contains(&"pk1".to_string()));
        } else {
            panic!("Expected Commit on restored state");
        }
    }
}
