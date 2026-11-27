use crate::nostr::wallet_registry::{Storage, WalletRegistry};
use crate::nostr::{Event, Filter, Limits, Tag};
use async_trait::async_trait;
use serde_json::Value;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ── Constants ──

pub const TEST_WALLET_SK: &str = "0101010101010101010101010101010101010101010101010101010101010101";
pub const TEST_WALLET_PK: &str = "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f";

/// A stable 64-character lowercase hex value named by `label`, for filter
/// values, which the relay accepts only in that form.
pub fn hex_key(label: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(label.as_bytes()))
}

// ── Clock seam ──

thread_local! {
    pub static TEST_TIME: Cell<u64> = const { Cell::new(1700000000) };
}

pub fn test_now() -> u64 {
    TEST_TIME.with(|t| t.get())
}

pub fn set_test_time(t: u64) {
    TEST_TIME.with(|c| c.set(t));
}

// ── Engine factory ──

pub fn new_test_engine() -> crate::nostr::engine::NostrEngine<MockStorage> {
    crate::nostr::engine::NostrEngine::new_with_storage(MockStorage::new(), test_limits(), test_now)
}

/// Relay limits for tests: the production defaults, except that info-cache
/// expiry is off.
///
/// Fixtures publish info events with tiny `created_at` values (1000) against a
/// pinned clock of `1_700_000_000`, so any real time-to-live would treat every
/// one of them as expired and bury the behaviour under test. Tests about
/// expiry spell out `info_ttl_seconds` themselves.
pub fn test_limits() -> Limits {
    Limits {
        info_ttl_seconds: 0,
        ..Limits::default()
    }
}

// ── MockStorage ──

pub struct MockStorage {
    pub data: Arc<Mutex<HashMap<String, Value>>>,
    pub fail_put_batch: bool,
}

impl MockStorage {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
            fail_put_batch: false,
        }
    }

    pub fn with_fail() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
            fail_put_batch: true,
        }
    }
}

#[async_trait(?Send)]
impl Storage for MockStorage {
    async fn get(&self, key: &str) -> Option<Value> {
        self.data.lock().unwrap().get(key).cloned()
    }
    async fn put_batch(
        &self,
        entries: HashMap<String, Value>,
    ) -> Result<(), crate::nostr::StorageError> {
        if self.fail_put_batch {
            return Err(crate::nostr::StorageError::from("mock storage unavailable"));
        }
        if entries.len() > crate::nostr::wallet_registry::MAX_STORAGE_BATCH {
            return Err(crate::nostr::StorageError::from(
                "too many entries in one put",
            ));
        }
        let mut data = self.data.lock().unwrap();
        for (k, v) in entries {
            data.insert(k, v);
        }
        Ok(())
    }
    async fn delete_batch(&self, keys: Vec<String>) {
        let mut data = self.data.lock().unwrap();
        for k in keys {
            data.remove(&k);
        }
    }
    async fn list_page(
        &self,
        prefix: &str,
        start_after: Option<&str>,
        limit: usize,
    ) -> Vec<(String, Value)> {
        let data = self.data.lock().unwrap();
        let mut entries: Vec<(String, Value)> = data
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .filter(|(k, _)| start_after.is_none_or(|after| k.as_str() > after))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // Key order, as Durable Object storage lists.
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries.truncate(limit);
        entries
    }
}

// ── Event factory ──

pub fn test_event(id: &str, pubkey: &str, kind: u64, tags: Vec<Tag>, created_at: u64) -> Event {
    Event {
        id: id.into(),
        pubkey: pubkey.into(),
        kind,
        tags,
        content: "test".into(),
        sig: "sig".into(),
        created_at,
    }
}

// ── Hibernation simulator ──

pub async fn simulate_hibernation(original: &MockStorage) -> WalletRegistry<MockStorage> {
    let snapshot = original.data.lock().unwrap().clone();
    let new_storage = MockStorage::new();
    new_storage.data.lock().unwrap().extend(snapshot);
    // Same limits and clock as the engine under test: the point of the
    // simulation is that only the in-memory index is lost.
    WalletRegistry::new(new_storage, test_limits(), test_now)
}

// ── Storage seed helpers ──

pub async fn seed_subscription(
    storage: &MockStorage,
    conn_id: u32,
    sub_id: &str,
    pk: &str,
    filters: Vec<Filter>,
) {
    let mut entries = HashMap::new();
    entries.insert(format!("pk:{pk}"), serde_json::json!(vec![conn_id]));
    let mut subs = std::collections::HashMap::new();
    subs.insert(sub_id.to_string(), serde_json::to_value(filters).unwrap());
    entries.insert(
        format!("conn:{conn_id}"),
        serde_json::json!({
            "subscriptions": subs,
            "info_event": null
        }),
    );
    storage.put_batch(entries).await.unwrap();
}
