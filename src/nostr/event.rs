use std::collections::HashSet;
use k256::schnorr::signature::hazmat::PrehashVerifier;
use k256::schnorr::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::nostr::{RelayError, Limits, Tag};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
/// A NIP-01 Nostr event: the domain type the relay validates, stores and
/// routes. `id` and `sig` are hex strings; see [`Event::verify`] for the
/// full validation contract.
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
    /// Computes the NIP-01 event ID for the given fields: the SHA-256 hash
    /// of the JSON array `[0, pubkey, created_at, kind, tags, content]`.
    /// Returns both the hex ID and the digest bytes (needed to verify the
    /// Schnorr signature).
    ///
    /// # Errors
    ///
    /// Returns [`RelayError::SerializationError`] if the canonical array
    /// cannot be serialized.
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

    /// Public keys this event is addressed to: the author's own pubkey plus
    /// every `p` tag target (used to route NWC request/response events).
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

    /// Full relay-side validation: allowed kinds, required `p`/`e` tags for
    /// NWC kinds, content length against `limits`, timestamp window
    /// (±900 s / not older than one year), recomputed event ID, and Schnorr
    /// signature verification.
    ///
    /// # Errors
    ///
    /// Returns the matching [`RelayError`] variant for the first failed
    /// check (`InvalidKind`, `MissingTag`, `LimitExceeded`,
    /// `TimestampTooFar`, `InvalidId`, `MalformedHex`, `InvalidSignature`).
    pub fn verify(&self, current_time: u64, limits: &Limits) -> Result<(), RelayError> {
        
        // Enforce allowed kinds
        match self.kind {
            5 | 13194 | 23194 | 23195 | 23196 | 23197 => {}
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

        let verifying_key = VerifyingKey::from_slice(&pubkey_bytes)
            .map_err(|_| RelayError::MalformedHex("public key format".into()))?;

        let signature = Signature::try_from(sig_bytes.as_slice())
            .map_err(|_| RelayError::MalformedHex("signature format".into()))?;

        verifying_key
            .verify_prehash(&id_bytes, &signature)
            .map_err(|_| RelayError::InvalidSignature)
    }
}

#[cfg(test)]
#[path = "event_test.rs"]
mod event_test;