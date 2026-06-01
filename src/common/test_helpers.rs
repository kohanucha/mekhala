use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use async_trait::async_trait;
use serde_json::Value;
use crate::nostr::Limits;
use crate::nostr::wallet_registry::{WalletRegistry, Storage};

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
}

#[async_trait(?Send)]
impl Storage for MockStorage {
    async fn get(&self, key: &str) -> Option<Value> {
        self.data.lock().unwrap().get(key).cloned()
    }
    async fn put_batch(&self, entries: HashMap<String, Value>) -> Result<(), String> {
        if self.fail_put_batch {
            return Err("mock storage unavailable".into());
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
}

/// Simulates DO hibernation by cloning storage state and creating a fresh
/// WalletRegistry from the snapshot (in-memory state is lost).
pub async fn simulate_hibernation(original: &MockStorage) -> WalletRegistry<MockStorage> {
    let snapshot = original.data.lock().unwrap().clone();
    let new_storage = MockStorage::new();
    new_storage.put_batch(snapshot).await.unwrap();
    WalletRegistry::new(new_storage, Limits::default())
}
