use crate::domain::{Event, Filter, RelayError};
use crate::runtime::Platform;

/// Messages sent by the client to the relay
#[derive(Debug)]
pub enum ClientMessage {
    Event(Event),
    Req(String, Vec<Filter>),
    Close(String),
}

impl ClientMessage {
    /// Parses a JSON string into a ClientMessage.
    pub fn from_json(text: &str) -> Result<Self, RelayError> {
        let arr: Vec<serde_json::Value> =
            serde_json::from_str(text).map_err(|e| RelayError::ParseError(e.to_string()))?;

        if arr.is_empty() {
            return Err(RelayError::ParseError("empty message array".into()));
        }

        let cmd = arr[0]
            .as_str()
            .ok_or_else(|| RelayError::ParseError("command is not a string".into()))?;

        match cmd {
            "EVENT" => {
                if arr.len() < 2 {
                    return Err(RelayError::ParseError("missing event object".into()));
                }
                let event: Event = serde_json::from_value(arr[1].clone())?;
                Ok(ClientMessage::Event(event))
            }
            "REQ" => {
                if arr.len() < 3 {
                    return Err(RelayError::ParseError(
                        "missing subscription ID or filters".into(),
                    ));
                }
                let sub_id = arr[1]
                    .as_str()
                    .ok_or_else(|| RelayError::ParseError("sub_id is not a string".into()))?
                    .to_string();
                let mut filters = Vec::new();
                for val in arr.iter().skip(2) {
                    let filter: Filter = serde_json::from_value(val.clone())?;
                    filters.push(filter);
                }
                Ok(ClientMessage::Req(sub_id, filters))
            }
            "CLOSE" => {
                if arr.len() < 2 {
                    return Err(RelayError::ParseError("missing subscription ID".into()));
                }
                let sub_id = arr[1]
                    .as_str()
                    .ok_or_else(|| RelayError::ParseError("sub_id is not a string".into()))?
                    .to_string();
                Ok(ClientMessage::Close(sub_id))
            }
            _ => Err(RelayError::ParseError(format!("unknown command: {}", cmd))),
        }
    }
}

/// Messages sent by the relay to the client
pub enum RelayMessage {
    Ok(String, bool, String),
    Event(String, Event),
    Eose(String),
    Notice(String),
    Closed(String, String),
}

impl RelayMessage {
    /// Serializes a RelayMessage into a JSON string.
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

pub fn handle_get_info() -> Result<worker::Response, worker::Error> {
    let info = serde_json::json!({
        "supported_nips": [1, 11, 47]
    });
    Platform::create_cors_response(worker::Response::from_json(&info)?)
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
}
