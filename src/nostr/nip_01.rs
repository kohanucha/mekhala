use crate::nostr::{Event, Filter};
use serde::Deserialize;
use serde_json::value::RawValue;

/// A fully validated NIP-01 client message after [`ClientMessage::from_json`].
#[derive(Debug, Clone)]
pub enum ClientMessage {
    /// `["EVENT", event]` — publish an event.
    Event(Event),
    /// `["REQ", sub_id, filter, ...]` — subscribe with one or more filters.
    Req(String, Vec<Filter>),
    /// `["CLOSE", sub_id]` — end a subscription.
    Close(String),
}

#[derive(Deserialize)]
struct PartialEvent {
    id: String,
}

/// Cheap pre-parse of an incoming message used to reject oversized or
/// malformed payloads before paying for full validation. Currently only
/// recognizes `EVENT` messages, exposing the event's `id`.
pub enum PartialClientMessage {
    Event(String), // Returns the ID if it looks like an event
}

impl PartialClientMessage {
    /// Best-effort parse; `None` when the text is not a recognizable
    /// NIP-01 `EVENT` array.
    pub fn from_json(text: &str) -> Option<Self> {
        let arr: Vec<&RawValue> = serde_json::from_str(text).ok()?;
        if arr.len() < 2 {
            return None;
        }

        let msg_type: String = serde_json::from_str(arr[0].get()).ok()?;
        if msg_type == "EVENT" {
            let event: PartialEvent = serde_json::from_str(arr[1].get()).ok()?;
            return Some(Self::Event(event.id));
        }
        None
    }
}

impl ClientMessage {
    /// Parses a NIP-01 client message array (full validation included).
    ///
    /// # Errors
    ///
    /// Returns a descriptive message string when the text is not valid
    /// JSON, not an array, or fails NIP-01 framing rules.
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
                let sub_id: String =
                    serde_json::from_str(arr[1].get()).map_err(|e| e.to_string())?;
                let mut filters = Vec::new();
                for value in &arr[2..] {
                    let filter: Filter =
                        serde_json::from_str(value.get()).map_err(|e| e.to_string())?;
                    filters.push(filter);
                }
                Ok(Self::Req(sub_id, filters))
            }
            "CLOSE" if arr.len() >= 2 => {
                let sub_id: String =
                    serde_json::from_str(arr[1].get()).map_err(|e| e.to_string())?;
                Ok(Self::Close(sub_id))
            }
            v => Err(format!("unknown message type: {v}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// A relay-to-client NIP-01 message; serialized to the WebSocket with
/// [`RelayMessage::to_json`]. The relay only ever sends these, so there is no
/// parser.
pub enum RelayMessage {
    /// `OK` — acknowledgment of an accepted/rejected event.
    Ok(String, bool, String),
    /// `EVENT` — an event delivered for a subscription.
    Event(String, crate::nostr::Event),
    /// `EOSE` — end of stored events for a subscription.
    Eose(String),
    /// `NOTICE` — human-readable relay notice.
    Notice(String),
    /// `CLOSED` — relay ended a subscription (e.g. connection lost).
    Closed(String, String),
}

impl RelayMessage {
    /// Serializes this message to its NIP-01 JSON wire form.
    pub fn to_json(&self) -> String {
        match self {
            Self::Ok(id, ok, msg) => serde_json::json!(["OK", id, ok, msg]).to_string(),
            Self::Event(sub_id, event) => serde_json::json!(["EVENT", sub_id, event]).to_string(),
            Self::Eose(sub_id) => serde_json::json!(["EOSE", sub_id]).to_string(),
            Self::Notice(msg) => serde_json::json!(["NOTICE", msg]).to_string(),
            Self::Closed(sub_id, msg) => serde_json::json!(["CLOSED", sub_id, msg]).to_string(),
        }
    }

    /// One short log line for this message, without the event's content,
    /// tags, signature or full keys.
    pub fn log_summary(&self) -> String {
        use crate::util::short;
        match self {
            Self::Ok(id, ok, msg) if msg.is_empty() => format!("OK id={} {ok}", short(id, 8)),
            Self::Ok(id, ok, msg) => format!("OK id={} {ok}: {msg}", short(id, 8)),
            Self::Event(sub_id, event) => format!(
                "EVENT sub={} kind={} id={}",
                short(sub_id, 16),
                event.kind,
                short(&event.id, 8)
            ),
            Self::Eose(sub_id) => format!("EOSE sub={}", short(sub_id, 16)),
            Self::Notice(msg) => format!("NOTICE {msg}"),
            Self::Closed(sub_id, msg) => format!("CLOSED sub={}: {msg}", short(sub_id, 16)),
        }
    }
}

#[cfg(test)]
#[path = "nip_01_test.rs"]
mod nip_01_test;
