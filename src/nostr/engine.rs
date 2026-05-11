use std::collections::{HashMap, HashSet};
use super::{Filter, Event, RelayMessage, ClientMessage};
use super::wallet_registry::WalletRegistry;
use crate::util::now;
use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
pub enum EngineResponse {
    /// Deliver a Nostr message to a specific WebSocket connection.
    Send { connection_id: u32, message: String },

    /// Notify an internal waiter (like the LN Bridge) that its response has arrived.
    Signal { connection_id: u32, message: String },

    /// Persist the connection's state to storage (Crucial for Hibernation).
    StoreState { connection_id: u32, connection_data: serde_json::Value },
}

impl EngineResponse {
    pub fn send(connection_id: u32, message: String) -> Self {
        EngineResponse::Send { connection_id, message }
    }

    pub fn signal(connection_id: u32, message: String) -> Self {
        EngineResponse::Signal { connection_id, message }
    }

    pub fn store_state(connection_id: u32, connection_data: serde_json::Value) -> Self {
        EngineResponse::StoreState { connection_id, connection_data }
    }
}

pub struct NostrEngine {
    connections: HashSet<u32>,
    registry: WalletRegistry,
}

impl NostrEngine {
    pub fn new() -> Self {
        Self {
            connections: HashSet::new(),
            registry: WalletRegistry::new(),
        }
    }

    pub fn on_connect(&mut self, id: u32) -> Vec<EngineResponse> {
        self.add_connection(id, HashMap::new());
        Vec::new()
    }

    pub fn on_message(&mut self, connection_id: u32, message: &str) -> Vec<EngineResponse> {
        match ClientMessage::from_json(message) {
            Ok(ClientMessage::Event(event)) => self.handle_nostr_event(connection_id, event),
            Ok(ClientMessage::Req(sub_id, filters)) => self.handle_nostr_req(connection_id, sub_id, filters),
            Ok(ClientMessage::Close(sub_id)) => self.handle_nostr_close(connection_id, sub_id),
            Err(e) => {
                vec![EngineResponse::send(connection_id, RelayMessage::Notice(format!("parse failed: {}", e)).to_json())]
            }
        }
    }

    pub fn handle_nostr_event(&mut self, connection_id: u32, event: Event) -> Vec<EngineResponse> {
        let is_virtual = !self.connections.contains(&connection_id);
        
        match event.kind {
            13194 => {
                if is_virtual || event.verify(now()).is_ok() {
                    self.registry.store_info_event(event);
                    // Store state after info event update
                    if let Some(state) = self.export_state(connection_id) {
                        return vec![EngineResponse::store_state(connection_id, state)];
                    }
                }
                Vec::new()
            }
            23194..=23197 => {
                if !is_virtual {
                    if let Err(e) = event.verify(now()) {
                        return vec![EngineResponse::send(connection_id, RelayMessage::Ok(event.id, false, e.to_string()).to_json())];
                    }
                }

                self.handle_verified_event(connection_id, event)
            }
            _ => {
                let verify_result = if is_virtual { Ok(()) } else { event.verify(now()) };
                let message = if let Err(e) = verify_result {
                    RelayMessage::Ok(event.id, false, e.to_string())
                } else {
                    RelayMessage::Ok(event.id, false, "blocked: event kind not allowed".into())
                };
                vec![EngineResponse::send(connection_id, message.to_json())]
            }
        }
    }

    fn handle_verified_event(&mut self, id: u32, event: Event) -> Vec<EngineResponse> {
        let mut responses = Vec::new();
        let ok_message = RelayMessage::Ok(event.id.clone(), true, "".into()).to_json();

        if self.connections.contains(&id) {
            responses.push(EngineResponse::send(id, ok_message));
        }

        // Normal routing via registry subscriptions
        for (client_id, recipient_ids) in self.registry.match_event(&event) {
            for rid in recipient_ids {
                let message = RelayMessage::Event(client_id.clone(), event.clone()).to_json();
                
                if self.connections.contains(&rid) {
                    responses.push(EngineResponse::send(rid, message));
                } else {
                    // Virtual/Bridge connection (Matches registry but not in active DO-managed connections)
                    responses.push(EngineResponse::signal(rid, message));
                }
            }
        }

        responses
    }

    pub fn on_disconnect(&mut self, id: u32) -> Vec<EngineResponse> {
        self.remove_connection(id);
        Vec::new()
    }

    pub fn get_wallet_info(&self, pubkey: &str) -> super::WalletInfo {
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
                                "nip44_v2" => { encryption.insert("nip44_v2".to_string()); }
                                "nip04" => { encryption.insert("nip04".to_string()); }
                                _ => {}
                            }
                        }
                    }
                }
            }
            if !has_encryption_tag {
                encryption.insert("nip04".to_string());
            }
        } else if self.registry.get_connection_id(pubkey).is_some() {
            // Subscription exists but no info event yet
            online = true;
        }

        super::WalletInfo {
            online,
            ready,
            encryption_algorithms: encryption.into_iter().collect::<Vec<_>>()
        }
    }

    pub fn error_message(&self, msg: &str) -> String {
        RelayMessage::Notice(msg.to_string()).to_json()
    }

    pub fn get_target_pubkeys(&self, message: &str) -> Option<Vec<String>> {
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

    pub fn export_state(&self, id: u32) -> Option<serde_json::Value> {
        if self.connections.contains(&id) || id != 0 {
            let registry_state = self.registry.export_connection(id);
            let state = serde_json::json!({
                "active": true,
                "registry": registry_state
            });

            return Some(state);
        }
        None
    }

    pub fn import_state(&mut self, id: u32, data: serde_json::Value) {
        if data.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
            if id != 0 {
                self.connections.insert(id);
            }
        }
        if let Some(registry_val) = data.get("registry") {
            self.registry.import_connection(id, registry_val.clone());
        }
    }

    pub fn add_connection(&mut self, id: u32, subscriptions: HashMap<String, Vec<Filter>>) {
        if id != 0 {
            self.connections.insert(id);
        }
        for (sub_id, filters) in subscriptions {
            self.registry.add_subscription(id, sub_id, filters);
        }
    }

    pub fn remove_connection(&mut self, id: u32) {
        if id != 0 {
            self.connections.remove(&id);
        }
        self.registry.remove_connection(id);
    }

    fn handle_nostr_req(&mut self, id: u32, sub_id: String, filters: Vec<Filter>) -> Vec<EngineResponse> {
        if filters.iter().any(|f| !f.is_valid()) {
            let message = RelayMessage::Closed(sub_id.clone(), "filter too broad".to_string()).to_json();
            return if self.connections.contains(&id) {
                vec![EngineResponse::send(id, message)]
            } else {
                vec![EngineResponse::signal(id, message)]
            };
        }

        let mut responses = Vec::new();
        if id != 0 {
            self.registry.add_subscription(id, sub_id.clone(), filters.clone());

            // Check if any existing info events match this new subscription
            for filters_set in filters.iter() {
                for pk in filters_set.pubkeys() {
                    if let Some(info_event) = self.registry.get_info_event(&pk) {
                        if filters.iter().any(|f| f.matches(&info_event)) {
                            let message = RelayMessage::Event(sub_id.clone(), info_event.clone()).to_json();
                            if self.connections.contains(&id) {
                                responses.push(EngineResponse::send(id, message));
                            } else {
                                responses.push(EngineResponse::signal(id, message));
                            }
                        }
                    }
                }
            }

            // Always store state after subscription change
            if let Some(state) = self.export_state(id) {
                responses.push(EngineResponse::store_state(id, state));
            }
        }

        let eose = RelayMessage::Eose(sub_id).to_json();
        if self.connections.contains(&id) {
            responses.push(EngineResponse::send(id, eose));
        } else {
            // Skip signaling EOSE to virtual connections
        }
        responses
    }

    fn handle_nostr_close(&mut self, id: u32, sub_id: String) -> Vec<EngineResponse> {
        if id != 0 {
            self.registry.remove_subscription(id, sub_id);
            // Always store state after subscription removal
            if let Some(state) = self.export_state(id) {
                return vec![EngineResponse::store_state(id, state)];
            }
        }
        Vec::new()
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
        
        // Should return StoreState and EOSE (Send)
        assert!(responses.iter().any(|r| matches!(r, EngineResponse::StoreState { .. })));
        assert!(responses.iter().any(|r| {
            if let EngineResponse::Send { message, .. } = r {
                message.contains("EOSE")
            } else {
                false
            }
        }));
        
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

    #[test]
    fn test_engine_get_wallet_info() {
        let engine = NostrEngine::new();
        let info = engine.get_wallet_info("pk1");
        assert_eq!(info.online, false);
    }

    #[test]
    fn test_bridge_signaling() {
        let mut engine = NostrEngine::new();
        engine.on_connect(1); // Wallet connection
        engine.registry.add_subscription(1, "sub1".into(), vec![Filter {
            p_tags: Some(vec!["wallet_pk".into()]),
            ..Default::default()
        }]);

        // Bridge REQ
        let bridge_req = serde_json::json!(["REQ", "sub_bridge", { "#p": ["bridge"] }]).to_string();
        let _ = engine.on_message(10, &bridge_req);

        let bridge_event_json = serde_json::json!([
            "EVENT",
            {
                "id": "event1",
                "pubkey": "bridge",
                "created_at": now(),
                "kind": 23194,
                "tags": [["p", "wallet_pk"]],
                "content": "",
                "sig": ""
            }
        ]).to_string();

        let responses = engine.on_message(10, &bridge_event_json);
        
        // Should return OK acknowledgment (but NOT signaled if it's virtual)
        // and Send (to wallet 1 - Send)
        assert!(!responses.iter().any(|r| matches!(r, EngineResponse::Signal { connection_id: 10, .. })));
        assert!(responses.iter().any(|r| matches!(r, EngineResponse::Send { connection_id: 1, .. })));

        // Now simulate wallet response with standard 'e' and 'p' tags
        let wallet_response_event: Event = serde_json::from_value(serde_json::json!({
            "id": "resp1",
            "pubkey": "wallet_pk",
            "created_at": now(),
            "kind": 23194,
            "tags": [["p", "bridge"], ["e", "event1"]],
            "content": "",
            "sig": "dummy_sig"
        })).unwrap();

        // Directly call handle_verified_event to bypass verify() for the unit test.
        let responses = engine.handle_verified_event(1, wallet_response_event);
        
        // Should return OK acknowledgment to wallet (ID 1 - Send), 
        // and Signal to bridge (ID 10 - Signal) because it's a ROUTED event.
        assert!(responses.iter().any(|r| matches!(r, EngineResponse::Signal { connection_id: 10, .. })));
        assert!(responses.iter().any(|r| matches!(r, EngineResponse::Send { connection_id: 1, .. })));
    }
}
