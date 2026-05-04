use crate::nostr::{Event, Filter};

#[derive(Debug, Clone)]
pub enum ClientMessage {
    Event(Event),
    Req(String, Vec<Filter>),
    Close(String),
}

impl ClientMessage {
    pub fn from_json(text: &str) -> Result<Self, String> {
        let arr: Vec<serde_json::Value> = serde_json::from_str(text).map_err(|e| e.to_string())?;

        if arr.is_empty() {
            return Err("empty message".to_string());
        }

        match arr[0].as_str() {
            Some("EVENT") if arr.len() >= 2 => {
                let event: Event = serde_json::from_value(arr[1].clone()).map_err(|e| e.to_string())?;
                Ok(Self::Event(event))
            }
            Some("REQ") if arr.len() >= 3 => {
                let sub_id = arr[1].as_str().ok_or("invalid sub_id")?.to_string();
                let mut filters = Vec::new();
                for value in &arr[2..] {
                    let filter: Filter = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
                    filters.push(filter);
                }
                Ok(Self::Req(sub_id, filters))
            }
            Some("CLOSE") if arr.len() >= 2 => {
                let sub_id = arr[1].as_str().ok_or("invalid sub_id")?.to_string();
                Ok(Self::Close(sub_id))
            }
            Some(v) => Err(format!("unknown message type: {}", v)),
            None => Err("missing message type".to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum RelayMessage {
    Ok(String, bool, String),
    Event(String, crate::nostr::Event),
    Eose(String),
    Notice(String),
    Closed(String, String),
}

impl RelayMessage {
    pub fn to_json(&self) -> String {
        match self {
            Self::Ok(id, ok, msg) => serde_json::json!(["OK", id, ok, msg]).to_string(),
            Self::Event(sub_id, event) => serde_json::json!(["EVENT", sub_id, event]).to_string(),
            Self::Eose(sub_id) => serde_json::json!(["EOSE", sub_id]).to_string(),
            Self::Notice(msg) => serde_json::json!(["NOTICE", msg]).to_string(),
            Self::Closed(sub_id, msg) => serde_json::json!(["CLOSED", sub_id, msg]).to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_message_parsing() {
        let event_json = r#"["EVENT",{"id":"id","pubkey":"pk","created_at":1000,"kind":1,"tags":[],"content":"hi","sig":"sig"}]"#;
        let msg = ClientMessage::from_json(event_json).unwrap();
        match msg {
            ClientMessage::Event(e) => assert_eq!(e.content, "hi"),
            _ => panic!("Expected Event"),
        }

        let req_json = r#"["REQ","sub1",{"authors":["pk1"]}]"#;
        let msg = ClientMessage::from_json(req_json).unwrap();
        match msg {
            ClientMessage::Req(id, filters) => {
                assert_eq!(id, "sub1");
                assert_eq!(filters.len(), 1);
            }
            _ => panic!("Expected Req"),
        }

        let close_json = r#"["CLOSE","sub1"]"#;
        let msg = ClientMessage::from_json(close_json).unwrap();
        match msg {
            ClientMessage::Close(id) => assert_eq!(id, "sub1"),
            _ => panic!("Expected Close"),
        }
    }

    #[test]
    fn test_relay_message_serialization() {
        assert_eq!(
            RelayMessage::Notice("hi".into()).to_json(),
            r#"["NOTICE","hi"]"#
        );
        assert_eq!(
            RelayMessage::Eose("sub1".into()).to_json(),
            r#"["EOSE","sub1"]"#
        );
        assert_eq!(
            RelayMessage::Ok("id1".into(), true, "".into()).to_json(),
            r#"["OK","id1",true,""]"#
        );
    }

    #[test]
    fn test_relay_message_event_serialization() {
        let event = crate::nostr::Event {
            id: "test_id".into(),
            pubkey: "test_pubkey".into(),
            created_at: 1234567890,
            kind: 1,
            tags: vec![],
            content: "test content".into(),
            sig: "test_sig".into(),
        };
        let msg = RelayMessage::Event("sub1".into(), event);
        let json = msg.to_json();
        assert!(json.starts_with(r#"["EVENT","sub1","#));
    }

    #[test]
    fn test_relay_message_closed_serialization() {
        assert_eq!(
            RelayMessage::Closed("sub1".into(), "reason".into()).to_json(),
            r#"["CLOSED","sub1","reason"]"#
        );
    }
}
