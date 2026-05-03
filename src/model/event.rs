use k256::schnorr::signature::hazmat::PrehashVerifier;
use k256::schnorr::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::model::error::RelayError;
use crate::model::limits::Limits;

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
    pub fn verify(&self, current_time: u64, limits: &Limits) -> Result<(), RelayError> {
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