use k256::schnorr::signature::hazmat::PrehashVerifier;
use k256::schnorr::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

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
#[derive(Debug, PartialEq, Clone)]
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
    /// Verifies generic Nostr event integrity (timestamp, ID, signature).
    pub fn verify(&self, current_time: u64, limits: &Limits) -> Result<(), RelayError> {
        // 1. Verify limits
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

        // 2. Verify timestamp limits (Replay protection)
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

        // 3. Verify ID (NIP-01)
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

        // 4. Verify Signature (NIP-01)
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

    /// Validates if the filter complies with generic array length limits.
    pub fn is_valid(&self, limits: &Limits) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use k256::schnorr::signature::hazmat::PrehashSigner;
    use k256::schnorr::SigningKey;
    use sha2::{Digest, Sha256};

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
            1,
            vec![],
            "test",
            None,
        );
        assert!(event.verify(1712160000, &Limits::default()).is_ok());
    }

    #[test]
    fn test_event_verify_invalid_sig() {
        let mut event = create_test_event(
            "0101010101010101010101010101010101010101010101010101010101010101",
            1,
            vec![],
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

        let event_future = create_test_event(sk, 1, vec![], "test", Some(now + 960));
        assert!(matches!(
            event_future.verify(now, &Limits::default()),
            Err(RelayError::TimestampTooFar(_))
        ));

        let event_old = create_test_event(sk, 1, vec![], "test", Some(now - 35_000_000));
        assert!(matches!(
            event_old.verify(now, &Limits::default()),
            Err(RelayError::TimestampTooFar(_))
        ));
    }

    #[test]
    fn test_filter_matches() {
        let event = Event {
            id: "id1".into(),
            pubkey: "pub1".into(),
            created_at: 1000,
            kind: 1,
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

        // Match Authors
        assert!(Filter {
            authors: Some(vec!["pub1".into()]),
            ..Default::default()
        }
        .matches(&event));
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
        let event_heavy_tags = create_test_event(sk, 1, tags, "test", Some(now));
        assert!(matches!(
            event_heavy_tags.verify(now, &Limits::default()),
            Err(RelayError::LimitExceeded(_))
        ));

        // Test content too large
        let large_content = "a".repeat(32769);
        let event_large_content = create_test_event(sk, 1, vec![], &large_content, Some(now));
        assert!(matches!(
            event_large_content.verify(now, &Limits::default()),
            Err(RelayError::LimitExceeded(_))
        ));

        // Test filter item limit
        let f_too_many_ids = Filter {
            ids: Some(vec!["a".into(); 101]),
            ..Default::default()
        };
        assert!(!f_too_many_ids.is_valid(&Limits::default()));
    }
}
