use k256::schnorr::signature::hazmat::PrehashVerifier;
use k256::schnorr::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Nostr Event Kinds
pub const KIND_NWC_INFO: u64 = 13194;
pub const KIND_NWC_REQUEST: u64 = 23194;
pub const KIND_NWC_RESPONSE: u64 = 23195;
pub const KIND_NWC_NOTIFICATION_1: u64 = 23196;
pub const KIND_NWC_NOTIFICATION_2: u64 = 23197;

const ALLOWED_KINDS: &[u64] = &[
    KIND_NWC_INFO,
    KIND_NWC_REQUEST,
    KIND_NWC_RESPONSE,
    KIND_NWC_NOTIFICATION_1,
    KIND_NWC_NOTIFICATION_2,
];

/// Security Limits
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Limits {
    pub max_filter_items: usize,
    pub max_event_tags: usize,
    pub max_content_length: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_filter_items: 100,
            max_event_tags: 100,
            max_content_length: 32768,
        }
    }
}

/// Custom error types for the relay
#[derive(Debug, PartialEq)]
pub enum RelayError {
    InvalidKind,
    TimestampTooFar(String),
    MissingTag(String),
    InvalidId,
    InvalidSignature,
    MalformedHex(String),
    SerializationError(String),
    ParseError(String),
    LimitExceeded(String),
}

impl fmt::Display for RelayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKind => write!(f, "blocked: event kind not allowed"),
            Self::TimestampTooFar(m) => write!(f, "invalid: {}", m),
            Self::MissingTag(m) => write!(f, "invalid: missing {}", m),
            Self::InvalidId => write!(f, "invalid: event ID mismatch"),
            Self::InvalidSignature => write!(f, "invalid: signature verification failed"),
            Self::MalformedHex(m) => write!(f, "invalid: malformed {}", m),
            Self::SerializationError(e) => write!(f, "error: serialization failed: {}", e),
            Self::ParseError(e) => write!(f, "error: parse failed: {}", e),
            Self::LimitExceeded(m) => write!(f, "rejected: {}", m),
        }
    }
}

impl From<serde_json::Error> for RelayError {
    fn from(e: serde_json::Error) -> Self {
        Self::SerializationError(e.to_string())
    }
}

impl From<hex::FromHexError> for RelayError {
    fn from(e: hex::FromHexError) -> Self {
        Self::MalformedHex(e.to_string())
    }
}

/// A standard Nostr Event
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Event {
    pub id: String,
    pub pubkey: String,
    pub created_at: u64,
    pub kind: u64,
    pub tags: Vec<Vec<serde_json::Value>>,
    pub content: String,
    pub sig: String,
}

impl Event {
    /// Verifies the event's kind, timestamp, ID, and signature.
    pub fn verify(&self, current_time: u64, limits: &Limits) -> Result<(), RelayError> {
        // 1. Verify Allowed Kinds
        if !ALLOWED_KINDS.contains(&self.kind) {
            return Err(RelayError::InvalidKind);
        }

        // 2. Verify limits
        if self.tags.len() > limits.max_event_tags {
            return Err(RelayError::LimitExceeded(format!(
                "too many tags (max {})",
                limits.max_event_tags
            )));
        }
        if self.content.len() > limits.max_content_length {
            return Err(RelayError::LimitExceeded(format!(
                "content too large (max {} bytes)",
                limits.max_content_length
            )));
        }

        // 3. Verify timestamp limits (Replay protection)
        if self.created_at > current_time + 900 {
            return Err(RelayError::TimestampTooFar(
                "event creation date is too far off from the current time".into(),
            ));
        }
        if self.created_at < current_time - 31_536_000 {
            return Err(RelayError::TimestampTooFar(
                "event creation date is too old".into(),
            ));
        }

        // 3. Verify NIP-47 constraints (strict tag enforcement)
        match self.kind {
            KIND_NWC_REQUEST | KIND_NWC_NOTIFICATION_1 | KIND_NWC_NOTIFICATION_2 => {
                if !self.has_tag("p") {
                    return Err(RelayError::MissingTag("#p tag".into()));
                }
            }
            KIND_NWC_RESPONSE => {
                if !self.has_tag("p") || !self.has_tag("e") {
                    return Err(RelayError::MissingTag("#p or #e tag".into()));
                }
            }
            _ => {}
        }

        // 4. Verify ID (NIP-01)
        let serialized = serde_json::to_string(&(
            0,
            &self.pubkey,
            self.created_at,
            self.kind,
            &self.tags,
            &self.content,
        ))
        .map_err(|e| RelayError::SerializationError(e.to_string()))?;

        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        let id_bytes = hasher.finalize();

        let expected_id_bytes = hex::decode(&self.id)?;
        if expected_id_bytes != id_bytes.as_ref() as &[u8] {
            return Err(RelayError::InvalidId);
        }

        // 5. Verify Signature (NIP-01)
        let pubkey_bytes = hex::decode(&self.pubkey)?;
        let sig_bytes = hex::decode(&self.sig)?;

        let verifying_key = VerifyingKey::from_bytes(&pubkey_bytes)
            .map_err(|_| RelayError::MalformedHex("public key format".into()))?;

        let signature = Signature::try_from(sig_bytes.as_slice())
            .map_err(|_| RelayError::MalformedHex("signature format".into()))?;

        verifying_key
            .verify_prehash(&id_bytes, &signature)
            .map_err(|_| RelayError::InvalidSignature)
    }

    /// Checks if the event has a specific tag name.
    pub fn has_tag(&self, tag_name: &str) -> bool {
        self.tags
            .iter()
            .any(|t| t.len() >= 2 && t[0].as_str() == Some(tag_name))
    }
}

/// A filter for subscribing to events
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Filter {
    pub ids: Option<Vec<String>>,
    pub authors: Option<Vec<String>>,
    pub kinds: Option<Vec<u64>>,
    #[serde(rename = "#p")]
    pub p_tags: Option<Vec<String>>,
    #[serde(rename = "#e")]
    pub e_tags: Option<Vec<String>>,
    pub since: Option<u64>,
    pub until: Option<u64>,
}

impl Filter {
    /// Checks if a filter matches a given event.
    pub fn matches(&self, event: &Event) -> bool {
        if let Some(ids) = &self.ids {
            if !ids.contains(&event.id) {
                return false;
            }
        }
        if let Some(authors) = &self.authors {
            if !authors.contains(&event.pubkey) {
                return false;
            }
        }
        if let Some(kinds) = &self.kinds {
            if !kinds.contains(&event.kind) {
                return false;
            }
        }
        if let Some(p_tags) = &self.p_tags {
            let has_match = event.tags.iter().any(|t| {
                t.len() >= 2
                    && t[0].as_str() == Some("p")
                    && t[1]
                        .as_str()
                        .map_or(false, |val| p_tags.iter().any(|s| s == val))
            });
            if !has_match {
                return false;
            }
        }
        if let Some(e_tags) = &self.e_tags {
            let has_match = event.tags.iter().any(|t| {
                t.len() >= 2
                    && t[0].as_str() == Some("e")
                    && t[1]
                        .as_str()
                        .map_or(false, |val| e_tags.iter().any(|s| s == val))
            });
            if !has_match {
                return false;
            }
        }
        if let Some(since) = self.since {
            if event.created_at < since {
                return false;
            }
        }
        if let Some(until) = self.until {
            if event.created_at > until {
                return false;
            }
        }
        true
    }

    /// Validates if the filter is narrowed enough for NWC kinds.
    pub fn is_valid(&self, limits: &Limits) -> bool {
        // 1. Enforce array length limits to prevent CPU exhaustion
        if let Some(ids) = &self.ids {
            if ids.len() > limits.max_filter_items {
                return false;
            }
        }
        if let Some(authors) = &self.authors {
            if authors.len() > limits.max_filter_items {
                return false;
            }
        }
        if let Some(kinds) = &self.kinds {
            if kinds.len() > limits.max_filter_items {
                return false;
            }
        }
        if let Some(p_tags) = &self.p_tags {
            if p_tags.len() > limits.max_filter_items {
                return false;
            }
        }
        if let Some(e_tags) = &self.e_tags {
            if e_tags.len() > limits.max_filter_items {
                return false;
            }
        }

        // 2. Protocol strictness: Require NWC kinds
        let kinds = match &self.kinds {
            Some(k) if !k.is_empty() => k,
            _ => return false, // Kinds MUST be present and not empty
        };

        // All requested kinds MUST be NWC kinds
        if kinds.iter().any(|k| !ALLOWED_KINDS.contains(k)) {
            return false;
        }

        // 3. Privacy + Performance: Require narrowing for sensitive kinds
        let has_narrowing = self.authors.as_ref().map_or(false, |v| !v.is_empty())
            || self.p_tags.as_ref().map_or(false, |v| !v.is_empty())
            || self.e_tags.as_ref().map_or(false, |v| !v.is_empty())
            || self.ids.as_ref().map_or(false, |v| !v.is_empty());

        if !has_narrowing {
            return false;
        }

        true
    }

    /// Returns all pubkeys (authors + p_tags) mentioned in this filter.
    pub fn pubkeys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        if let Some(authors) = &self.authors {
            keys.extend(authors.clone());
        }
        if let Some(p_tags) = &self.p_tags {
            keys.extend(p_tags.clone());
        }
        keys
    }
}

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
    crate::utils::create_cors_response(worker::Response::from_json(&info)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::schnorr::signature::hazmat::PrehashSigner;
    use k256::schnorr::SigningKey;

    fn create_test_event(
        priv_key_hex: &str,
        kind: u64,
        tags: Vec<Vec<serde_json::Value>>,
        content: &str,
        timestamp: Option<u64>,
    ) -> Event {
        let priv_key_bytes = hex::decode(priv_key_hex).unwrap();
        let signing_key = SigningKey::from_bytes(&priv_key_bytes).unwrap();
        let verifying_key = signing_key.verifying_key();
        let pubkey = hex::encode(verifying_key.to_bytes());
        let created_at = timestamp.unwrap_or(1712160000);

        let serialized =
            serde_json::to_string(&(0, &pubkey, created_at, kind, &tags, content)).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        let id_bytes = hasher.finalize();
        let id = hex::encode(id_bytes);
        let signature = signing_key.sign_prehash(&id_bytes).unwrap();
        let sig = hex::encode(signature.to_bytes());

        Event {
            id,
            pubkey,
            created_at,
            kind,
            tags,
            content: content.into(),
            sig,
        }
    }

    #[test]
    fn test_event_verify_valid() {
        let event = create_test_event(
            "0101010101010101010101010101010101010101010101010101010101010101",
            KIND_NWC_REQUEST,
            vec![vec!["p".into(), "pub".into()]],
            "test",
            None,
        );
        assert!(event.verify(1712160000, &Limits::default()).is_ok());
    }

    #[test]
    fn test_event_verify_invalid_sig() {
        let mut event = create_test_event(
            "0101010101010101010101010101010101010101010101010101010101010101",
            KIND_NWC_REQUEST,
            vec![vec!["p".into(), "pub".into()]],
            "test",
            None,
        );
        event.sig = "0".repeat(128);
        assert!(matches!(
            event.verify(1712160000, &Limits::default()),
            Err(RelayError::MalformedHex(_)) | Err(RelayError::InvalidSignature)
        ));
    }

    #[test]
    fn test_event_verify_timestamp_limits() {
        let sk = "0101010101010101010101010101010101010101010101010101010101010101";
        let now = 1712160000;

        let event_future = create_test_event(
            sk,
            KIND_NWC_REQUEST,
            vec![vec!["p".into(), "pub".into()]],
            "test",
            Some(now + 960),
        );
        assert!(matches!(
            event_future.verify(now, &Limits::default()),
            Err(RelayError::TimestampTooFar(_))
        ));

        let event_old = create_test_event(
            sk,
            KIND_NWC_REQUEST,
            vec![vec!["p".into(), "pub".into()]],
            "test",
            Some(now - 35_000_000),
        );
        assert!(matches!(
            event_old.verify(now, &Limits::default()),
            Err(RelayError::TimestampTooFar(_))
        ));
    }

    #[test]
    fn test_client_message_parsing() {
        let raw_event = r#"["EVENT",{"id":"id","pubkey":"pub","created_at":100,"kind":23194,"tags":[],"content":"","sig":"sig"}]"#;
        assert!(ClientMessage::from_json(raw_event).is_ok());

        let raw_req = r#"["REQ","sub",{"kinds":[23194]}]"#;
        assert!(ClientMessage::from_json(raw_req).is_ok());

        let malformed = "invalid json";
        assert!(ClientMessage::from_json(malformed).is_err());
    }

    #[test]
    fn test_filter_matches() {
        let event = Event {
            id: "id1".into(),
            pubkey: "pub1".into(),
            created_at: 1000,
            kind: 23194,
            tags: vec![
                vec!["p".into(), "p_val".into()],
                vec!["e".into(), "e_val".into()],
            ],
            content: "hi".into(),
            sig: "sig".into(),
        };

        // Match all
        assert!(Filter::default().matches(&event));

        // Match IDs
        assert!(Filter {
            ids: Some(vec!["id1".into()]),
            ..Default::default()
        }
        .matches(&event));
        assert!(!Filter {
            ids: Some(vec!["id2".into()]),
            ..Default::default()
        }
        .matches(&event));

        // Match Authors
        assert!(Filter {
            authors: Some(vec!["pub1".into()]),
            ..Default::default()
        }
        .matches(&event));
        assert!(!Filter {
            authors: Some(vec!["pub2".into()]),
            ..Default::default()
        }
        .matches(&event));

        // Match Kinds
        assert!(Filter {
            kinds: Some(vec![23194]),
            ..Default::default()
        }
        .matches(&event));
        assert!(!Filter {
            kinds: Some(vec![23195]),
            ..Default::default()
        }
        .matches(&event));

        // Match p-tags
        assert!(Filter {
            p_tags: Some(vec!["p_val".into()]),
            ..Default::default()
        }
        .matches(&event));
        assert!(!Filter {
            p_tags: Some(vec!["wrong".into()]),
            ..Default::default()
        }
        .matches(&event));

        // Match e-tags
        assert!(Filter {
            e_tags: Some(vec!["e_val".into()]),
            ..Default::default()
        }
        .matches(&event));
        assert!(!Filter {
            e_tags: Some(vec!["wrong".into()]),
            ..Default::default()
        }
        .matches(&event));

        // Match Since/Until
        assert!(Filter {
            since: Some(900),
            ..Default::default()
        }
        .matches(&event));
        assert!(!Filter {
            since: Some(1100),
            ..Default::default()
        }
        .matches(&event));
        assert!(Filter {
            until: Some(1100),
            ..Default::default()
        }
        .matches(&event));
        assert!(!Filter {
            until: Some(900),
            ..Default::default()
        }
        .matches(&event));
    }

    #[test]
    fn test_filter_is_valid_nwc_strict() {
        // Must have kinds
        let f_no_kinds = Filter {
            authors: Some(vec!["a".into()]),
            ..Default::default()
        };
        assert!(!f_no_kinds.is_valid(&Limits::default()));

        // Must be NWC kinds
        let f_invalid_kind = Filter {
            kinds: Some(vec![1]),
            authors: Some(vec!["a".into()]),
            ..Default::default()
        };
        assert!(!f_invalid_kind.is_valid(&Limits::default()));

        // Must have narrowing
        let f_no_narrowing = Filter {
            kinds: Some(vec![KIND_NWC_REQUEST]),
            ..Default::default()
        };
        assert!(!f_no_narrowing.is_valid(&Limits::default()));

        // Valid
        let f_valid = Filter {
            kinds: Some(vec![KIND_NWC_REQUEST]),
            authors: Some(vec!["a".into()]),
            ..Default::default()
        };
        assert!(f_valid.is_valid(&Limits::default()));
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
    fn test_security_limits() {
        let sk = "0101010101010101010101010101010101010101010101010101010101010101";
        let now = 1712160000;

        // Test too many tags
        let mut tags = Vec::new();
        for i in 0..101 {
            tags.push(vec!["t".into(), i.to_string().into()]);
        }
        let event_heavy_tags = create_test_event(sk, KIND_NWC_REQUEST, tags, "test", Some(now));
        assert!(matches!(
            event_heavy_tags.verify(now, &Limits::default()),
            Err(RelayError::LimitExceeded(_))
        ));

        // Test content too large
        let large_content = "a".repeat(32769);
        let event_large_content = create_test_event(
            sk,
            KIND_NWC_REQUEST,
            vec![vec!["p".into(), "pub".into()]],
            &large_content,
            Some(now),
        );
        assert!(matches!(
            event_large_content.verify(now, &Limits::default()),
            Err(RelayError::LimitExceeded(_))
        ));

        // Test global subscription rejection
        assert!(!Filter::default().is_valid(&Limits::default()));

        // Test filter item limit
        let f_too_many_ids = Filter {
            kinds: Some(vec![KIND_NWC_REQUEST]),
            ids: Some(vec!["a".into(); 101]),
            ..Default::default()
        };
        assert!(!f_too_many_ids.is_valid(&Limits::default()));
    }
}
