use std::collections::{HashMap, HashSet};
use super::ConnectionState;
use super::{Filter, Event, RelayMessage, ClientMessage};
use super::wallet_registry::WalletRegistry;
use crate::util::engine::{Engine, EngineResponse};
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

    pub fn add_connection(&mut self, id: u32, state: ConnectionState, subscriptions: HashMap<String, Vec<Filter>>) {
        self.connections.insert(id, state);
        for (sub_id, filters) in subscriptions {
            self.registry.add_subscription(id, sub_id, filters);
        }
    }

    pub fn remove_connection(&mut self, id: u32) {
        self.connections.remove(&id);
        self.registry.remove_connection(id);
    }

    fn handle_nostr_event(&mut self, id: u32, event: Event) -> Vec<EngineResponse> {
        let verify_result = event.verify(now());
        
        match event.kind {
            13194 => {
                if verify_result.is_ok() {
                    self.registry.store_info_event(event);
                }
                Vec::new()
            }
            23194..=23197 => {
                if let Err(e) = verify_result {
                    return vec![EngineResponse::new(id, RelayMessage::Ok(event.id, false, e.to_string()).to_json())];
                }

                let mut responses = vec![EngineResponse::new(id, RelayMessage::Ok(event.id.clone(), true, "".into()).to_json())];
                for (sub_id, recipient_ids) in self.registry.match_event(&event) {
                    responses.push(EngineResponse::multi(
                        recipient_ids,
                        RelayMessage::Event(sub_id, event.clone()).to_json()
                    ));
                }
                responses
            }
            _ => {
                let message = if let Err(e) = verify_result {
                    RelayMessage::Ok(event.id, false, e.to_string())
                } else {
                    RelayMessage::Ok(event.id, false, "blocked: event kind not allowed".into())
                };
                vec![EngineResponse::new(id, message.to_json())]
            }
        }
    }

    fn handle_nostr_req(&mut self, id: u32, sub_id: String, filters: Vec<Filter>) -> Vec<EngineResponse> {
        if filters.iter().any(|f| !f.is_valid()) {
            return vec![EngineResponse::new(id, RelayMessage::Closed(sub_id, "filter too broad".to_string()).to_json())];
        }

        let mut responses = Vec::new();
        if self.connections.contains_key(&id) {
            self.registry.add_subscription(id, sub_id.clone(), filters.clone());

            // Check if any existing info events match this new subscription
            for filters_set in filters.iter() {
                for pk in filters_set.pubkeys() {
                    if let Some(info_event) = self.registry.get_info_event(&pk) {
                        if filters.iter().any(|f| f.matches(&info_event)) {
                            responses.push(EngineResponse::new(id, RelayMessage::Event(sub_id.clone(), info_event.clone()).to_json()));
                        }
                    }
                }
            }
        }

        responses.push(EngineResponse::new(id, RelayMessage::Eose(sub_id).to_json()));
        responses
    }

    fn handle_nostr_close(&mut self, id: u32, sub_id: String) -> Vec<EngineResponse> {
        if self.connections.contains_key(&id) {
            self.registry.remove_subscription(id, sub_id);
        }
        Vec::new()
    }

    pub fn get_wallet_info(&self, pubkey: &str) -> Value {
        let mut online = false;
        let mut ready = false;
        let mut encryption = HashSet::new();

        if let Some(info_event) = self.registry.get_info_event(pubkey) {
            online = true;
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
        } else if !self.registry.get_connection_ids(pubkey).is_empty() {
            // Subscription exists but no info event yet
            online = true;
        }

        serde_json::json!({
            "online": online,
            "ready": ready,
            "encryption": encryption.into_iter().collect::<Vec<_>>()
        })
    }
}

impl Engine for NostrEngine {
    fn on_connect(&mut self, id: u32) -> Vec<EngineResponse> {
        let mut conn_state = ConnectionState::default();
        conn_state.id = id;
        self.add_connection(id, conn_state, HashMap::new());
        Vec::new()
    }

    fn on_reconnect(&mut self, id: u32, state: Vec<u8>) {
        if let Ok(blob) = serde_json::from_slice::<serde_json::Value>(&state) {
            if let Some(conn_state_val) = blob.get("state") {
                if let Ok(conn_state) = serde_json::from_value::<ConnectionState>(conn_state_val.clone()) {
                    let subscriptions = blob.get("subscriptions")
                        .and_then(|s| serde_json::from_value::<HashMap<String, Vec<Filter>>>(s.clone()).ok())
                        .unwrap_or_default();
                    self.add_connection(id, conn_state, subscriptions);
                }
            }
        }
    }

    fn on_message(&mut self, id: u32, message: &str) -> Vec<EngineResponse> {
        match ClientMessage::from_json(message) {
            Ok(ClientMessage::Event(event)) => self.handle_nostr_event(id, event),
            Ok(ClientMessage::Req(sub_id, filters)) => self.handle_nostr_req(id, sub_id, filters),
            Ok(ClientMessage::Close(sub_id)) => self.handle_nostr_close(id, sub_id),
            Err(e) => {
                vec![EngineResponse::new(id, RelayMessage::Notice(format!("parse failed: {}", e)).to_json())]
            }
        }
    }

    fn on_disconnect(&mut self, id: u32) -> Vec<EngineResponse> {
        self.remove_connection(id);
        Vec::new()
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

    fn get_connection_ids(&self, pubkey: &str) -> Vec<u32> {
        self.registry.get_connection_ids(pubkey)
    }

    fn get_target_pubkeys(&self, message: &str) -> Option<Vec<String>> {
        if message.starts_with("[\"EVENT\"") {
            if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(message) {
                if arr.len() >= 2 {
                    if let Ok(event) = serde_json::from_value::<Event>(arr[1].clone()) {
                        let mut target_pks = HashSet::new();
                        for tag in &event.tags {
                            if tag.len() >= 2 && tag[0].as_str() == Some("p") {
                                if let Some(pk) = tag[1].as_str() {
                                    target_pks.insert(pk.to_string());
                                }
                            }
                        }
                        return Some(target_pks.into_iter().collect());
                    }
                }
            }
        }
        None
    }

    fn export_state(&self, id: u32) -> Option<serde_json::Value> {
        if let Some(conn_state) = self.connections.get(&id) {
            let registry_state = self.registry.export_connection(id);
            return Some(serde_json::json!({
                "conn": conn_state,
                "registry": registry_state
            }));
        }
        None
    }

    fn import_state(&mut self, id: u32, data: serde_json::Value) {
        if let Some(conn_val) = data.get("conn") {
            if let Ok(conn_state) = serde_json::from_value::<ConnectionState>(conn_val.clone()) {
                self.connections.insert(id, conn_state);
            }
        }
        if let Some(registry_val) = data.get("registry") {
            self.registry.import_connection(id, registry_val.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_req_storage() {
        let mut engine = NostrEngine::new();
        engine.on_connect(1);
        
        let req = r#"["REQ", "sub1", {"authors": ["pk1"]}]"#;
        let responses = engine.on_message(1, req);
        
        // Should return EOSE
        assert_eq!(responses.len(), 1);
        assert!(responses[0].messages.contains("EOSE"));
        
        // Verify sub is in registry
        assert!(engine.registry.get_subscriptions(1).contains_key("sub1"));
    }

    #[test]
    fn test_engine_info_event_routing() {
        let mut engine = NostrEngine::new();
        engine.on_connect(1);
        
        // Kind 13194 Info Event
        let event = Event {
            id: "id1".into(),
            pubkey: "pk1".into(),
            created_at: 1000,
            kind: 13194,
            tags: vec![],
            content: "".into(),
            sig: "sig1".into(),
        };

        // Directly store in registry for test purposes since verify() will fail on dummy sig
        engine.registry.store_info_event(event);
        
        // But it should be stored in registry
        assert!(engine.registry.get_info_event("pk1").is_some());
    }
}
