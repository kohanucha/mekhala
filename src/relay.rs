use k256::schnorr::Signature;
use k256::schnorr::VerifyingKey;
use k256::schnorr::signature::hazmat::PrehashVerifier;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    pub fn verify(&self) -> bool {
        // 1. Verify Allowed Kinds (NIP-01, NIP-47)
        let allowed_kinds = [0, 1, 13194, 23194, 23195, 23196, 23197];
        if !allowed_kinds.contains(&self.kind) {
            return false;
        }

        // 2. Verify NIP-47 constraints (p-tag enforcement)
        if self.kind == 23194 || self.kind == 23195 || self.kind == 23196 || self.kind == 23197 {
            let has_p_tag = self.tags.iter().any(|t| t.len() >= 2 && t[0] == "p");
            if !has_p_tag {
                return false;
            }
        }

        // 3. Verify ID (NIP-01)
        let serialized = match serde_json::to_string(&serde_json::json!([
            0,
            self.pubkey,
            self.created_at,
            self.kind,
            self.tags,
            self.content
        ])) {
            Ok(s) => s,
            Err(_) => return false,
        };

        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        let id_bytes = hasher.finalize();
        let id_hex = hex::encode(id_bytes);

        if id_hex != self.id {
            return false;
        }

        // 4. Verify Signature (NIP-01)
        let pubkey_bytes = match hex::decode(&self.pubkey) {
            Ok(b) => b,
            Err(_) => return false,
        };

        let sig_bytes = match hex::decode(&self.sig) {
            Ok(b) => b,
            Err(_) => return false,
        };

        let verifying_key = match VerifyingKey::from_bytes(&pubkey_bytes) {
            Ok(k) => k,
            Err(_) => return false,
        };

        let signature = match Signature::try_from(sig_bytes.as_slice()) {
            Ok(s) => s,
            Err(_) => return false,
        };

        verifying_key.verify_prehash(&id_bytes, &signature).is_ok()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Filter {
    pub ids: Option<Vec<String>>,
    pub authors: Option<Vec<String>>,
    pub kinds: Option<Vec<u64>>,
    #[serde(rename = "#p")]
    pub p_tags: Option<Vec<String>>,
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
            let event_p_tags: Vec<String> = event
                .tags
                .iter()
                .filter(|t| t.len() >= 2 && t[0] == "p")
                .map(|t| t[1].clone())
                .collect();
            if !p_tags.iter().any(|p| event_p_tags.contains(p)) {
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::schnorr::SigningKey;
    use k256::schnorr::signature::hazmat::PrehashSigner;

    #[test]
    fn test_event_verification() {
        // Use a fixed private key for testing
        let priv_key_hex = "0101010101010101010101010101010101010101010101010101010101010101";
        let priv_key_bytes = hex::decode(priv_key_hex).unwrap();
        let signing_key = SigningKey::from_bytes(&priv_key_bytes).unwrap();
        let verifying_key = signing_key.verifying_key();
        let pubkey = hex::encode(verifying_key.to_bytes());

        let created_at = 1712160000;
        let kind = 1;
        let tags: Vec<Vec<String>> = vec![];
        let content = "Hello, world!".into();

        let serialized = serde_json::json!([
            0,
            pubkey,
            created_at,
            kind,
            tags,
            content
        ]).to_string();

        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        let id_bytes = hasher.finalize();
        let id = hex::encode(id_bytes);

        let signature = signing_key.sign_prehash(&id_bytes).unwrap();
        let sig = hex::encode(signature.to_bytes());

        let event = Event {
            id,
            pubkey,
            created_at,
            kind,
            tags,
            content,
            sig,
        };

        assert!(event.verify());
    }

    #[test]
    fn test_nip47_p_tag_enforcement() {
        let event = Event {
            id: "1".into(),
            pubkey: "pub1".into(),
            created_at: 100,
            kind: 23194,
            tags: vec![], // Missing p-tag
            content: "".into(),
            sig: "".into(),
        };

        // Should fail because kind 23194 requires a p-tag
        assert!(!event.verify());

        let event_23196 = Event {
            id: "2".into(),
            pubkey: "pub1".into(),
            created_at: 100,
            kind: 23196,
            tags: vec![], // Missing p-tag
            content: "".into(),
            sig: "".into(),
        };

        // Should fail because kind 23196 requires a p-tag
        assert!(!event_23196.verify());
    }

    #[test]
    fn test_filter_matching() {
        let event = Event {
            id: "1".into(),
            pubkey: "pub1".into(),
            created_at: 100,
            kind: 23194,
            tags: vec![vec!["p".into(), "recipient1".into()]],
            content: "".into(),
            sig: "".into(),
        };

        let filter = Filter {
            ids: None,
            authors: None,
            kinds: Some(vec![23194]),
            p_tags: Some(vec!["recipient1".into()]),
            since: Some(50),
            until: Some(150),
            limit: None,
        };

        assert!(filter.matches(&event));

        let non_matching_filter = Filter {
            ids: None,
            authors: None,
            kinds: Some(vec![23194]),
            p_tags: Some(vec!["recipient2".into()]),
            since: None,
            until: None,
            limit: None,
        };

        assert!(!non_matching_filter.matches(&event));
    }
}
