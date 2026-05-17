use std::collections::HashSet;
use k256::schnorr::signature::hazmat::PrehashVerifier;
use k256::schnorr::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::nostr::{RelayError, Limits, Tag};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub id: String,
    pub pubkey: String,
    pub created_at: u64,
    pub kind: u64,
    pub tags: Vec<Tag>,
    pub content: String,
    pub sig: String,
}

impl Event {
    pub fn compute_id(pubkey: &str, created_at: u64, kind: u64, tags: &[Tag], content: &str) -> Result<(String, Vec<u8>), RelayError> {
        let serialized = serde_json::to_string(&(
            0,
            pubkey,
            created_at,
            kind,
            tags,
            content,
        ))
        .map_err(|e| RelayError::SerializationError(e.to_string()))?;

        let id_bytes = Sha256::digest(serialized.as_bytes());
        Ok((hex::encode(id_bytes), id_bytes.to_vec()))
    }

    pub fn target_pubkeys(&self) -> HashSet<String> {
        let mut keys = HashSet::new();
        keys.insert(self.pubkey.clone());
        for tag in &self.tags {
            if let Some(pk) = tag.pubkey() {
                keys.insert(pk.to_string());
            }
        }
        keys
    }

    pub fn verify(&self, current_time: u64, limits: &Limits) -> Result<(), RelayError> {
        
        // Enforce NWC-only kinds
        match self.kind {
            13194 | 23194 | 23195 | 23196 | 23197 => {}
            _ => return Err(RelayError::InvalidKind),
        }

        // Enforce tags for specific NWC kinds
        match self.kind {
            23195 => {
                let has_p = self.tags.iter().any(|t| t.is_p());
                let has_e = self.tags.iter().any(|t| t.is_e());
                if !has_p { return Err(RelayError::MissingTag("p".into())); }
                if !has_e { return Err(RelayError::MissingTag("e".into())); }
            }
            23196 | 23197 => {
                let has_p = self.tags.iter().any(|t| t.is_p());
                if !has_p { return Err(RelayError::MissingTag("p".into())); }
            }
            _ => {}
        }

        if self.tags.len() > limits.max_event_tags {
            return Err(RelayError::LimitExceeded(format!("too many tags (max {})", limits.max_event_tags)));
        }
        if self.content.len() > limits.max_content_length {
            return Err(RelayError::LimitExceeded(format!("content too large (max {} bytes)", limits.max_content_length)));
        }

        if self.created_at > current_time + 900 {
            return Err(RelayError::TimestampTooFar("event creation date is too far off from the current time".into()));
        }
        if self.created_at < current_time - 31_536_000 {
            return Err(RelayError::TimestampTooFar("event creation date is too old".into()));
        }

        let (computed_id, id_bytes) = Event::compute_id(&self.pubkey, self.created_at, self.kind, &self.tags, &self.content)?;
        if self.id != computed_id {
            return Err(RelayError::InvalidId);
        }

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_pubkeys_author_only() {
        let event = Event {
            id: "id".into(),
            pubkey: "pk1".into(),
            created_at: 0,
            kind: 1,
            tags: vec![],
            content: "".into(),
            sig: "".into(),
        };
        let keys = event.target_pubkeys();
        assert_eq!(keys.len(), 1);
        assert!(keys.contains("pk1"));
    }

    #[test]
    fn test_target_pubkeys_with_p_tags() {
        let event = Event {
            id: "id".into(),
            pubkey: "author".into(),
            created_at: 0,
            kind: 1,
            tags: vec![
                Tag::p("recipient1"),
                Tag::p("recipient2"),
                Tag::e("event_id"),
            ],
            content: "".into(),
            sig: "".into(),
        };
        let keys = event.target_pubkeys();
        assert_eq!(keys.len(), 3);
        assert!(keys.contains("author"));
        assert!(keys.contains("recipient1"));
        assert!(keys.contains("recipient2"));
    }

    #[test]
    fn test_target_pubkeys_deduplication() {
        let event = Event {
            id: "id".into(),
            pubkey: "pk1".into(),
            created_at: 0,
            kind: 1,
            tags: vec![
                Tag::P("pk1".into(), vec![]),
                Tag::p("pk2"),
                Tag::P("pk2".into(), vec![]),
            ],
            content: "".into(),
            sig: "".into(),
        };
        let keys = event.target_pubkeys();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains("pk1"));
        assert!(keys.contains("pk2"));
    }
}