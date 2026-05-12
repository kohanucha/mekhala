use std::collections::{HashMap, HashSet};
use super::{Filter, Event, RelayMessage, ClientMessage};
use super::wallet_registry::{WalletRegistry, InMemoryWalletRegistry};
use crate::util::now;
use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
pub enum EngineResponse {
    Send { connection_id: u32, message: String },
    StoreState { connection_id: u32, connection_data: serde_json::Value, pubkeys: HashSet<String> },
}

#[derive(Default, Clone, Copy)]
pub struct MessageFlags {
    pub is_internal: bool, // Skips OK/EOSE/CLOSED responses and skips emitting EngineResponse::StoreState
}

impl EngineResponse {
    pub fn send(connection_id: u32, message: String) -> Self {
        EngineResponse::Send { connection_id, message }
    }

    pub fn store_state(connection_id: u32, state: crate::nostr::wallet_registry::SavedState) -> Self {
        EngineResponse::StoreState { 
            connection_id,
            connection_data: state.json,
            pubkeys: state.pubkeys 
        }
    }
}

pub struct NostrEngine<R: WalletRegistry> {
    pub registry: R,
}

impl<R: WalletRegistry> NostrEngine<R> {
    pub fn new() -> NostrEngine<InMemoryWalletRegistry> {
        NostrEngine {
            registry: InMemoryWalletRegistry::new(),
        }
    }

    pub fn on_connect(&mut self, id: u32) -> Vec<EngineResponse> {
        self.add_connection(id, HashMap::new());
        Vec::new()
    }

    pub fn on_message(&mut self, connection_id: u32, message: &str, flags: MessageFlags) -> Vec<EngineResponse> {
        match ClientMessage::from_json(message) {
            Ok(ClientMessage::Event(event)) => self.handle_nostr_event(connection_id, event, flags),
            Ok(ClientMessage::Req(sub_id, filters)) => self.handle_nostr_req(connection_id, sub_id, filters, flags),
            Ok(ClientMessage::Close(sub_id)) => self.handle_nostr_close(connection_id, sub_id, flags),
            Err(e) => {
                if !flags.is_internal {
                    vec![EngineResponse::send(connection_id, RelayMessage::Notice(format!("parse failed: {}", e)).to_json())]
                } else {
                    Vec::new()
                }
            }
        }
    }

    pub fn handle_nostr_event(&mut self, connection_id: u32, event: Event, flags: MessageFlags) -> Vec<EngineResponse> {
        match event.kind {
            13194 => {
                if event.verify(now()).is_ok() {
                    self.registry.cache_info(event);
                    if !flags.is_internal {
                        if let Some(state) = self.registry.save(connection_id) {
                            return vec![EngineResponse::store_state(connection_id, state)];
                        }
                    }
                }
                Vec::new()
            }
            23194..=23197 => {
                if let Err(e) = event.verify(now()) {
                    if !flags.is_internal {
                        return vec![EngineResponse::send(connection_id, RelayMessage::Ok(event.id, false, e.to_string()).to_json())];
                    } else {
                        return Vec::new();
                    }
                }
                self.handle_verified_event(connection_id, event, flags)
            }
            _ => {
                if !flags.is_internal {
                    let message = if let Err(e) = event.verify(now()) {
                        RelayMessage::Ok(event.id, false, e.to_string())
                    } else {
                        RelayMessage::Ok(event.id, false, "blocked: event kind not allowed".into())
                    };
                    
                    vec![EngineResponse::send(connection_id, message.to_json())]
                } else {
                    Vec::new()
                }
            }
        }
    }

    fn handle_verified_event(&mut self, id: u32, event: Event, flags: MessageFlags) -> Vec<EngineResponse> {
        let mut responses = Vec::new();
        let ok_message = RelayMessage::Ok(event.id.clone(), true, "".into()).to_json();

        if !flags.is_internal {
            responses.push(EngineResponse::send(id, ok_message));
        }

        let matches: Vec<(String, Vec<u32>)> = self.registry.match_event(&event).collect();
        for (client_id, recipient_ids) in matches {
            for rid in recipient_ids {
                let message = RelayMessage::Event(client_id.clone(), event.clone()).to_json();
                responses.push(EngineResponse::send(rid, message));
            }
        }

        responses
    }

    pub fn on_disconnect(&mut self, id: u32) -> Vec<EngineResponse> {
        self.disconnect(id);
        Vec::new()
    }

    pub fn get_wallet_info(&self, pubkey: &str) -> super::WalletInfo {
        let mut online = false;
        let mut ready = false;
        let mut encryption = HashSet::new();

        if let Some(info_event) = self.registry.get_info(pubkey) {
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

    pub fn add_connection(&mut self, id: u32, subscriptions: HashMap<String, Vec<Filter>>) {
        for (sub_id, filters) in subscriptions {
            self.registry.subscribe(id, sub_id, filters);
        }
    }

    pub fn disconnect(&mut self, id: u32) {
        self.registry.disconnect(id);
    }

    fn handle_nostr_req(&mut self, id: u32, sub_id: String, filters: Vec<Filter>, flags: MessageFlags) -> Vec<EngineResponse> {
        if filters.iter().any(|f| !f.is_valid()) {
            let message = RelayMessage::Closed(sub_id.clone(), "filter too broad".to_string()).to_json();
            
            if !flags.is_internal {
                return vec![EngineResponse::send(id, message)];
            } else {
                return Vec::new();
            }
        }

        let mut responses = Vec::new();
        self.registry.subscribe(id, sub_id.clone(), filters.clone());

        for filters_set in filters.iter() {
            for pk in filters_set.pubkeys() {
                if let Some(info_event) = self.registry.get_info(&pk) {
                    if filters.iter().any(|f| f.matches(&info_event)) {
                        let message = RelayMessage::Event(sub_id.clone(), info_event.clone()).to_json();
                        
                        if !flags.is_internal {
                            responses.push(EngineResponse::send(id, message));
                        }
                    }
                }
            }
        }

        if !flags.is_internal {
            if let Some(state) = self.registry.save(id) {
                responses.push(EngineResponse::store_state(id, state));
            }
        }

        let eose = RelayMessage::Eose(sub_id).to_json();
        if !flags.is_internal {
            responses.push(EngineResponse::send(id, eose));
        }
        responses
    }

    fn handle_nostr_close(&mut self, id: u32, sub_id: String, flags: MessageFlags) -> Vec<EngineResponse> {
        self.registry.unsubscribe(id, sub_id);
        if !flags.is_internal {
            if let Some(state) = self.registry.save(id) {
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
        let mut engine = NostrEngine::<InMemoryWalletRegistry>::new();
        engine.on_connect(1);

        let req = r#"["REQ", "sub1", {"authors": ["pk1"]}]"#;
        let responses = engine.on_message(1, req, MessageFlags::default());

        assert!(responses.iter().any(|r| matches!(r, EngineResponse::StoreState { .. })));
        assert!(responses.iter().any(|r| {
            if let EngineResponse::Send { message, .. } = r {
                message.contains("EOSE")
            } else {
                false
            }
        }));

        assert!(engine.registry.get_subscriptions(1).contains_key("sub1"));
    }

    #[test]
    fn test_engine_info_event_routing() {
        let mut engine = NostrEngine::<InMemoryWalletRegistry>::new();
        engine.on_connect(1);

        let event = Event {
            id: "id1".into(),
            pubkey: "pk1".into(),
            created_at: 1000,
            kind: 13194,
            tags: vec![],
            content: "".into(),
            sig: "sig1".into(),
        };

        engine.registry.cache_info(event);

        assert!(engine.registry.get_info("pk1").is_some());
    }

    #[test]
    fn test_engine_get_wallet_info() {
        let engine = NostrEngine::<InMemoryWalletRegistry>::new();
        let info = engine.get_wallet_info("pk1");
        assert_eq!(info.online, false);
    }

    #[test]
    fn test_bridge_signaling() {
        let mut engine = NostrEngine::<InMemoryWalletRegistry>::new();
        engine.on_connect(1);
        let wallet_pk = "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f";
        engine.registry.subscribe(1, "sub1".into(), vec![Filter {
            p_tags: Some(vec![wallet_pk.into()]),
            ..Default::default()
        }]);

        let bridge_id = 100;
        let bridge_sk = "0202020202020202020202020202020202020202020202020202020202020202";

        // Create a valid signed event for the bridge
        let details = crate::nostr::nip_47::WalletConnectionDetails {
            wallet_pubkey: wallet_pk.to_string(),
            secret: bridge_sk.to_string(),
        };
        let connection = crate::nostr::nip_47::WalletConnection::new(details).unwrap();

        let bridge_req = serde_json::json!(["REQ", "sub_bridge", { "#p": [connection.my_pubkey] }]).to_string();
        let responses = engine.on_message(bridge_id, &bridge_req, MessageFlags { is_internal: true });

        // REQ should NOT send anything back to sender with suppress_acks (is_internal)
        assert!(!responses.iter().any(|r| matches!(r, EngineResponse::Send { connection_id: 100, .. })));

        // Create a valid signed event for the bridge
        let details = crate::nostr::nip_47::WalletConnectionDetails {
            wallet_pubkey: wallet_pk.to_string(),
            secret: bridge_sk.to_string(),
        };
        let connection = crate::nostr::nip_47::WalletConnection::new(details).unwrap();
        let bridge_event = connection.create_event(23194, "".into(), vec![vec![serde_json::json!("p"), serde_json::json!(wallet_pk)]]).unwrap();

        let bridge_event_json = serde_json::json!([
            "EVENT",
            bridge_event
        ]).to_string();

        let responses = engine.on_message(bridge_id, &bridge_event_json, MessageFlags { is_internal: true });

        // Event should be routed to connection 1
        assert!(responses.iter().any(|r| matches!(r, EngineResponse::Send { connection_id: 1, .. })));
        // EVENT should NOT send OK back to sender (is_internal)
        assert!(!responses.iter().any(|r| matches!(r, EngineResponse::Send { connection_id: 100, .. })));

        let wallet_response_event: Event = serde_json::from_value(serde_json::json!({
            "id": "resp1",
            "pubkey": "wallet_pk",
            "created_at": now(),
            "kind": 23194,
            "tags": [["p", "bridge"], ["e", "event1"]],
            "content": "",
            "sig": "dummy_sig"
        })).unwrap();
        // Since we use handle_verified_event directly for the response, sig verification is skipped there.
        // But for the sake of realism, we can use the bridge's pubkey.
        let mut wallet_response_event = wallet_response_event;
        wallet_response_event.tags = vec![vec![serde_json::json!("p"), serde_json::json!(connection.my_pubkey)], vec![serde_json::json!("e"), serde_json::json!(bridge_event.id)]];

        // Wallet response comes from connection 1
        let responses = engine.handle_verified_event(1, wallet_response_event, MessageFlags::default());

        // Routed EVENT SHOULD go to connection 100
        assert!(responses.iter().any(|r| {
            if let EngineResponse::Send { connection_id, message } = r {
                *connection_id == 100 && message.contains("resp1")
            } else {
                false
            }
        }));
    }

    #[test]
    fn test_virtual_connection_lifecycle() {
        let mut engine = NostrEngine::<InMemoryWalletRegistry>::new();
        engine.on_connect(1);

        let id = 100;
        engine.registry.subscribe(id, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]);

        let event = Event {
            id: "event1".into(),
            pubkey: "alice".into(),
            created_at: now(),
            kind: 23194,
            tags: vec![],
            content: "test".into(),
            sig: "sig".into(),
        };

        let responses = engine.handle_verified_event(2, event, MessageFlags::default());

        assert!(responses.iter().any(|r| {
            if let EngineResponse::Send { connection_id, message } = r {
                *connection_id == 100 && message.contains("event1")
            } else {
                false
            }
        }));

        engine.registry.disconnect(id);
        assert!(engine.registry.get_subscriptions(id).is_empty());
    }
}
