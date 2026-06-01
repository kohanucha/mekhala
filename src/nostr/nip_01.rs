use crate::nostr::{Event, Filter};
use serde_json::value::RawValue;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub enum ClientMessage {
    Event(Event),
    Req(String, Vec<Filter>),
    Close(String),
}

#[derive(Deserialize)]
struct PartialEvent {
    id: String,
}

pub enum PartialClientMessage {
    Event(String), // Returns the ID if it looks like an event
}

impl PartialClientMessage {
    pub fn from_json(text: &str) -> Option<Self> {
        let arr: Vec<&RawValue> = serde_json::from_str(text).ok()?;
        if arr.len() < 2 { return None; }

        let msg_type: String = serde_json::from_str(arr[0].get()).ok()?;
        if msg_type == "EVENT" {
            let event: PartialEvent = serde_json::from_str(arr[1].get()).ok()?;
            return Some(Self::Event(event.id));
        }
        None
    }
}

impl ClientMessage {
    pub fn from_json(text: &str) -> Result<Self, String> {
        let arr: Vec<&RawValue> = serde_json::from_str(text).map_err(|e| e.to_string())?;

        if arr.is_empty() {
            return Err("empty message".to_string());
        }

        let msg_type: String = serde_json::from_str(arr[0].get()).map_err(|e| e.to_string())?;

        match msg_type.as_str() {
            "EVENT" if arr.len() >= 2 => {
                let event: Event = serde_json::from_str(arr[1].get()).map_err(|e| e.to_string())?;
                Ok(Self::Event(event))
            }
            "REQ" if arr.len() >= 3 => {
                let sub_id: String = serde_json::from_str(arr[1].get()).map_err(|e| e.to_string())?;
                let mut filters = Vec::new();
                for value in &arr[2..] {
                    let filter: Filter = serde_json::from_str(value.get()).map_err(|e| e.to_string())?;
                    filters.push(filter);
                }
                Ok(Self::Req(sub_id, filters))
            }
            "CLOSE" if arr.len() >= 2 => {
                let sub_id: String = serde_json::from_str(arr[1].get()).map_err(|e| e.to_string())?;
                Ok(Self::Close(sub_id))
            }
            v => Err(format!("unknown message type: {}", v)),
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
#[path = "nip_01_test.rs"]
mod nip_01_test;
