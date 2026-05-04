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
