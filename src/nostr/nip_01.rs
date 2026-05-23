use crate::nostr::{Event, Filter};
use serde_json::value::RawValue;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub enum ClientMessage {
    Event(Event),
    Req(String, Vec<Filter>),
    Close(String),
}

/// Parse error carrying an optional event ID for targeted error responses.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub event_id: Option<String>,
    pub message: String,
}

impl ParseError {
    /// Convert parse error to engine responses for the originating connection.
    pub fn into_responses(self, connection_id: u32) -> Vec<crate::nostr::engine::EngineResponse> {
        use crate::nostr::engine::EngineResponse;
        use crate::nostr::RelayMessage;
        if let Some(id) = self.event_id {
            vec![EngineResponse::send(connection_id, RelayMessage::Ok(id, false, format!("parse failed: {}", self.message)))]
        } else {
            vec![EngineResponse::send(connection_id, RelayMessage::Notice(format!("parse failed: {}", self.message)))]
        }
    }
}

#[derive(Deserialize)]
struct PartialEvent {
    id: String,
}

impl ClientMessage {
    pub fn from_json(text: &str) -> Result<Self, ParseError> {
        let arr: Vec<&RawValue> = serde_json::from_str(text).map_err(|e| ParseError {
            event_id: None,
            message: e.to_string(),
        })?;

        if arr.is_empty() {
            return Err(ParseError {
                event_id: None,
                message: "empty message".into(),
            });
        }

        let msg_type: String = serde_json::from_str(arr[0].get()).map_err(|e| ParseError {
            event_id: None,
            message: e.to_string(),
        })?;

        // Helper to extract event ID from a failed EVENT parse
        let event_id = if msg_type == "EVENT" && arr.len() >= 2 {
            serde_json::from_str::<PartialEvent>(arr[1].get())
                .ok()
                .map(|e| e.id)
        } else {
            None
        };

        match msg_type.as_str() {
            "EVENT" if arr.len() >= 2 => {
                let event: Event = serde_json::from_str(arr[1].get()).map_err(|e| ParseError {
                    event_id,
                    message: e.to_string(),
                })?;
                Ok(Self::Event(event))
            }
            "REQ" if arr.len() >= 3 => {
                let sub_id: String = serde_json::from_str(arr[1].get()).map_err(|e| ParseError {
                    event_id: None,
                    message: e.to_string(),
                })?;
                let mut filters = Vec::new();
                for value in &arr[2..] {
                    let filter: Filter = serde_json::from_str(value.get()).map_err(|e| ParseError {
                        event_id: None,
                        message: e.to_string(),
                    })?;
                    filters.push(filter);
                }
                Ok(Self::Req(sub_id, filters))
            }
            "CLOSE" if arr.len() >= 2 => {
                let sub_id: String = serde_json::from_str(arr[1].get()).map_err(|e| ParseError {
                    event_id: None,
                    message: e.to_string(),
                })?;
                Ok(Self::Close(sub_id))
            }
            v => Err(ParseError {
                event_id,
                message: format!("unknown message type: {}", v),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayMessage {
    Ok(String, bool, String),
    Event(String, crate::nostr::Event),
    Eose(String),
    Notice(String),
    Closed(String, String),
}

impl RelayMessage {
    pub fn from_json(text: &str) -> Result<Self, String> {
        let arr: Vec<&RawValue> = serde_json::from_str(text).map_err(|e| e.to_string())?;

        if arr.is_empty() {
            return Err("empty message".to_string());
        }

        let msg_type: String = serde_json::from_str(arr[0].get()).map_err(|e| e.to_string())?;

        match msg_type.as_str() {
            "OK" if arr.len() >= 4 => {
                let id: String = serde_json::from_str(arr[1].get()).map_err(|e| e.to_string())?;
                let ok: bool = serde_json::from_str(arr[2].get()).map_err(|e| e.to_string())?;
                let msg: String = serde_json::from_str(arr[3].get()).map_err(|e| e.to_string())?;
                Ok(Self::Ok(id, ok, msg))
            }
            "EVENT" if arr.len() >= 3 => {
                let sub_id: String = serde_json::from_str(arr[1].get()).map_err(|e| e.to_string())?;
                let event: crate::nostr::Event = serde_json::from_str(arr[2].get()).map_err(|e| e.to_string())?;
                Ok(Self::Event(sub_id, event))
            }
            "EOSE" if arr.len() >= 2 => {
                let sub_id: String = serde_json::from_str(arr[1].get()).map_err(|e| e.to_string())?;
                Ok(Self::Eose(sub_id))
            }
            "NOTICE" if arr.len() >= 2 => {
                let msg: String = serde_json::from_str(arr[1].get()).map_err(|e| e.to_string())?;
                Ok(Self::Notice(msg))
            }
            "CLOSED" if arr.len() >= 3 => {
                let sub_id: String = serde_json::from_str(arr[1].get()).map_err(|e| e.to_string())?;
                let msg: String = serde_json::from_str(arr[2].get()).map_err(|e| e.to_string())?;
                Ok(Self::Closed(sub_id, msg))
            }
            v => Err(format!("unknown message type: {}", v)),
        }
    }

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

        // Parse error with event ID
        let bad_event = r#"["EVENT",{"id":"bad_id"}]"#;
        let err = ClientMessage::from_json(bad_event).unwrap_err();
        assert_eq!(err.event_id, Some("bad_id".into()));
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

    #[test]
    fn test_nip_11_info_structure() {
        let info = serde_json::json!({"supported_nips": [1, 11, 47]});
        assert!(info.is_object());
        let nips = info.get("supported_nips").and_then(|v| v.as_array()).unwrap();
        assert!(nips.contains(&serde_json::json!(1)));
        assert!(nips.contains(&serde_json::json!(11)));
        assert!(nips.contains(&serde_json::json!(47)));
    }
}
