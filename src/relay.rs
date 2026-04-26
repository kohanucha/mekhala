use k256::schnorr::Signature;
use k256::schnorr::VerifyingKey;
use k256::schnorr::signature::hazmat::PrehashVerifier;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const KIND_METADATA: u64 = 0;
pub const KIND_TEXT_NOTE: u64 = 1;
pub const KIND_NWC_INFO: u64 = 13194;
pub const KIND_NWC_REQUEST: u64 = 23194;
pub const KIND_NWC_RESPONSE: u64 = 23195;
pub const KIND_NWC_NOTIFICATION_1: u64 = 23196;
pub const KIND_NWC_NOTIFICATION_2: u64 = 23197;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Event {
    pub id: String,
    pub pubkey: String,
    pub created_at: u64,
    pub kind: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: String,
}

impl Event {
    pub fn verify(&self, current_time: u64) -> Result<(), String> {
        // 1. Verify Allowed Kinds (NIP-01, NIP-47)
        let allowed_kinds = [
            KIND_METADATA,
            KIND_TEXT_NOTE,
            KIND_NWC_INFO,
            KIND_NWC_REQUEST,
            KIND_NWC_RESPONSE,
            KIND_NWC_NOTIFICATION_1,
            KIND_NWC_NOTIFICATION_2,
        ];
        if !allowed_kinds.contains(&self.kind) {
            return Err("blocked: event kind not allowed".into());
        }

        // 2. Verify timestamp limits (Replay protection)
        // Future limit: 15 minutes
        if self.created_at > current_time + 900 {
            return Err("invalid: event creation date is too far off from the current time".into());
        }
        // Past limit: 1 year
        if self.created_at < current_time - 31_536_000 {
            return Err("invalid: event creation date is too old".into());
        }

        // 3. Verify NIP-47 constraints (strict tag enforcement)
        match self.kind {
            KIND_NWC_REQUEST | KIND_NWC_NOTIFICATION_1 | KIND_NWC_NOTIFICATION_2 => {
                if !self.has_tag("p") {
                    return Err("invalid: missing #p tag".into());
                }
            }
            KIND_NWC_RESPONSE => {
                if !self.has_tag("p") || !self.has_tag("e") {
                    return Err("invalid: missing #p or #e tag".into());
                }
            }
            _ => {}
        }

        // 4. Verify ID (NIP-01)
        let serialized = match serde_json::to_string(&serde_json::json!([
            0,
            self.pubkey,
            self.created_at,
            self.kind,
            self.tags,
            self.content
        ])) {
            Ok(s) => s,
            Err(_) => return Err("error: failed to serialize event for ID verification".into()),
        };

        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        let id_bytes = hasher.finalize();

        let expected_id_bytes = match hex::decode(&self.id) {
            Ok(b) => b,
            Err(_) => return Err("invalid: malformed event ID".into()),
        };

        if expected_id_bytes != id_bytes.as_ref() as &[u8] {
            return Err("invalid: event ID mismatch".into());
        }

        // 5. Verify Signature (NIP-01)
        let pubkey_bytes = match hex::decode(&self.pubkey) {
            Ok(b) => b,
            Err(_) => return Err("invalid: malformed public key".into()),
        };

        let sig_bytes = match hex::decode(&self.sig) {
            Ok(b) => b,
            Err(_) => return Err("invalid: malformed signature".into()),
        };

        let verifying_key = match VerifyingKey::from_bytes(&pubkey_bytes) {
            Ok(k) => k,
            Err(_) => return Err("invalid: invalid public key format".into()),
        };

        let signature = match Signature::try_from(sig_bytes.as_slice()) {
            Ok(s) => s,
            Err(_) => return Err("invalid: invalid signature format".into()),
        };

        if verifying_key.verify_prehash(&id_bytes, &signature).is_ok() {
            Ok(())
        } else {
            Err("invalid: signature verification failed".into())
        }
    }

    fn has_tag(&self, tag_name: &str) -> bool {
        self.tags.iter().any(|t| t.len() >= 2 && t[0] == tag_name)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
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
    pub limit: Option<usize>,
}

impl Filter {
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
                t.len() >= 2 && t[0] == "p" && p_tags.contains(&t[1])
            });
            if !has_match {
                return false;
            }
        }
        if let Some(e_tags) = &self.e_tags {
            let has_match = event.tags.iter().any(|t| {
                t.len() >= 2 && t[0] == "e" && e_tags.contains(&t[1])
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

    pub fn is_valid(&self) -> bool {
        // If the filter specifies NIP-47 request/response/notification kinds,
        // it MUST be narrowed by author, p-tag, or e-tag to prevent a global firehose.
        let nip47_kinds = [
            KIND_NWC_REQUEST,
            KIND_NWC_RESPONSE,
            KIND_NWC_NOTIFICATION_1,
            KIND_NWC_NOTIFICATION_2,
        ];
        if let Some(kinds) = &self.kinds {
            let requests_nip47 = kinds.iter().any(|k| nip47_kinds.contains(k));
            if requests_nip47 {
                let has_author = self.authors.as_ref().map(|a| !a.is_empty()).unwrap_or(false);
                let has_p_tag = self.p_tags.as_ref().map(|p| !p.is_empty()).unwrap_or(false);
                let has_e_tag = self.e_tags.as_ref().map(|e| !e.is_empty()).unwrap_or(false);
                if !has_author && !has_p_tag && !has_e_tag {
                    return false;
                }
            }
        }
        true
    }
}

#[derive(Debug)]
pub enum ClientMessage {
    Event(Event),
    Req(String, Vec<Filter>),
    Close(String),
}

impl ClientMessage {
    pub fn from_json(text: &str) -> Option<Self> {
        let arr: Vec<serde_json::Value> = serde_json::from_str(text).ok()?;
        if arr.is_empty() { return None; }
        
        match arr[0].as_str()? {
            "EVENT" => {
                if arr.len() < 2 { return None; }
                let event: Event = serde_json::from_value(arr[1].clone()).ok()?;
                Some(ClientMessage::Event(event))
            }
            "REQ" => {
                if arr.len() < 3 { return None; }
                let sub_id = arr[1].as_str()?.to_string();
                let mut filters = Vec::new();
                for i in 2..arr.len() {
                    let filter: Filter = serde_json::from_value(arr[i].clone()).ok()?;
                    filters.push(filter);
                }
                Some(ClientMessage::Req(sub_id, filters))
            }
            "CLOSE" => {
                if arr.len() < 2 { return None; }
                let sub_id = arr[1].as_str()?.to_string();
                Some(ClientMessage::Close(sub_id))
            }
            _ => None
        }
    }
}

pub enum RelayMessage {
    Ok(String, bool, String),
    Event(String, Event),
    Eose(String),
    Notice(String),
    Closed(String, String),
}

impl RelayMessage {
    pub fn to_json(&self) -> String {
        match self {
            RelayMessage::Ok(id, ok, msg) => {
                serde_json::json!(["OK", id, ok, msg]).to_string()
            }
            RelayMessage::Event(sub_id, event) => {
                serde_json::json!(["EVENT", sub_id, event]).to_string()
            }
            RelayMessage::Eose(sub_id) => {
                serde_json::json!(["EOSE", sub_id]).to_string()
            }
            RelayMessage::Notice(msg) => {
                serde_json::json!(["NOTICE", msg]).to_string()
            }
            RelayMessage::Closed(sub_id, msg) => {
                serde_json::json!(["CLOSED", sub_id, msg]).to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::schnorr::SigningKey;
    use k256::schnorr::signature::hazmat::PrehashSigner;

    fn create_test_event(priv_key_hex: &str, kind: u64, tags: Vec<Vec<String>>, content: &str, timestamp: Option<u64>) -> Event {
        let priv_key_bytes = hex::decode(priv_key_hex).unwrap();
        let signing_key = SigningKey::from_bytes(&priv_key_bytes).unwrap();
        let verifying_key = signing_key.verifying_key();
        let pubkey = hex::encode(verifying_key.to_bytes());
        let created_at = timestamp.unwrap_or(1712160000);

        let serialized = serde_json::json!([0, pubkey, created_at, kind, tags, content]).to_string();
        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        let id_bytes = hasher.finalize();
        let id = hex::encode(id_bytes);
        let signature = signing_key.sign_prehash(&id_bytes).unwrap();
        let sig = hex::encode(signature.to_bytes());

        Event { id, pubkey, created_at, kind, tags, content: content.into(), sig }
    }

    #[test]
    fn test_event_verify_valid() {
        let event = create_test_event("0101010101010101010101010101010101010101010101010101010101010101", KIND_TEXT_NOTE, vec![], "test", None);
        assert!(event.verify(1712160000).is_ok());
    }

    #[test]
    fn test_event_verify_invalid_sig() {
        let mut event = create_test_event("0101010101010101010101010101010101010101010101010101010101010101", KIND_TEXT_NOTE, vec![], "test", None);
        event.sig = "0".repeat(128);
        assert!(event.verify(1712160000).is_err());
    }

    #[test]
    fn test_event_verify_invalid_id() {
        let mut event = create_test_event("0101010101010101010101010101010101010101010101010101010101010101", KIND_TEXT_NOTE, vec![], "test", None);
        event.id = "0".repeat(64);
        assert!(event.verify(1712160000).is_err());
    }

    #[test]
    fn test_event_verify_malformed_hex() {
        let mut event = create_test_event("0101010101010101010101010101010101010101010101010101010101010101", KIND_TEXT_NOTE, vec![], "test", None);
        event.pubkey = "nothex".into();
        assert!(event.verify(1712160000).is_err());
    }

    #[test]
    fn test_event_verify_disallowed_kind() {
        let event = create_test_event("0101010101010101010101010101010101010101010101010101010101010101", 3, vec![], "test", None);
        assert!(event.verify(1712160000).is_err());
    }

    #[test]
    fn test_event_verify_timestamp_limits() {
        let sk = "0101010101010101010101010101010101010101010101010101010101010101";
        let now = 1712160000;
        
        // Future: +16 mins (960s)
        let event = create_test_event(sk, KIND_TEXT_NOTE, vec![], "test", Some(now + 960));
        assert!(event.verify(now).is_err());

        // Past: -1.1 years
        let event = create_test_event(sk, KIND_TEXT_NOTE, vec![], "test", Some(now - 35_000_000));
        assert!(event.verify(now).is_err());

        // Valid: +5 mins
        let event = create_test_event(sk, KIND_TEXT_NOTE, vec![], "test", Some(now + 300));
        assert!(event.verify(now).is_ok());
    }

    #[test]
    fn test_nip47_tags_enforcement() {
        let sk = "0101010101010101010101010101010101010101010101010101010101010101";
        let now = 1712160000;
        
        // 23194 missing p
        let e = create_test_event(sk, KIND_NWC_REQUEST, vec![], "", None);
        assert!(e.verify(now).is_err());
        
        // 23194 with p
        let e = create_test_event(sk, KIND_NWC_REQUEST, vec![vec!["p".into(), "pub".into()]], "", None);
        assert!(e.verify(now).is_ok());

        // 23195 missing e or p
        let e = create_test_event(sk, KIND_NWC_RESPONSE, vec![vec!["p".into(), "pub".into()]], "", None);
        assert!(e.verify(now).is_err());
        let e = create_test_event(sk, KIND_NWC_RESPONSE, vec![vec!["e".into(), "id".into()]], "", None);
        assert!(e.verify(now).is_err());
        
        // 23195 with both
        let e = create_test_event(sk, KIND_NWC_RESPONSE, vec![vec!["p".into(), "pub".into()], vec!["e".into(), "id".into()]], "", None);
        assert!(e.verify(now).is_ok());
    }

    #[test]
    fn test_client_message_parsing() {
        // Valid Event
        let raw_event = r#"["EVENT",{"id":"id","pubkey":"pub","created_at":100,"kind":1,"tags":[],"content":"","sig":"sig"}]"#;
        assert!(ClientMessage::from_json(raw_event).is_some());

        // Valid REQ
        let raw_req = r#"["REQ","sub",{"kinds":[1]}]"#;
        assert!(ClientMessage::from_json(raw_req).is_some());

        // Valid CLOSE
        let raw_close = r#"["CLOSE","sub"]"#;
        assert!(ClientMessage::from_json(raw_close).is_some());

        // Malformed
        assert!(ClientMessage::from_json("invalid").is_none());
        assert!(ClientMessage::from_json(r#"["EVENT"]"#).is_none());
    }

    #[test]
    fn test_filter_is_valid() {
        // Broad NWC
        let f = Filter { kinds: Some(vec![KIND_NWC_REQUEST]), ..Default::default() };
        assert!(!f.is_valid());

        // Narrowed NWC
        let f = Filter { kinds: Some(vec![KIND_NWC_REQUEST]), authors: Some(vec!["p".into()]), ..Default::default() };
        assert!(f.is_valid());

        // Broad non-NWC
        let f = Filter { kinds: Some(vec![KIND_TEXT_NOTE]), ..Default::default() };
        assert!(f.is_valid());
    }

    #[test]
    fn test_filter_matches() {
        let event = create_test_event("0101010101010101010101010101010101010101010101010101010101010101", KIND_TEXT_NOTE, vec![vec!["p".into(), "target".into()]], "hi", None);
        
        let f = Filter { kinds: Some(vec![KIND_TEXT_NOTE]), ..Default::default() };
        assert!(f.matches(&event));

        let f = Filter { p_tags: Some(vec!["target".into()]), ..Default::default() };
        assert!(f.matches(&event));

        let f = Filter { authors: Some(vec!["wrong".into()]), ..Default::default() };
        assert!(!f.matches(&event));
    }

    #[test]
    fn test_relay_message_serialization() {
        let msg = RelayMessage::Closed("sub".into(), "reason".into());
        assert_eq!(msg.to_json(), r#"["CLOSED","sub","reason"]"#);

        let msg = RelayMessage::Ok("id".into(), false, "error".into());
        assert_eq!(msg.to_json(), r#"["OK","id",false,"error"]"#);
    }
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            ids: None,
            authors: None,
            kinds: None,
            p_tags: None,
            e_tags: None,
            since: None,
            until: None,
            limit: None,
        }
    }
}
