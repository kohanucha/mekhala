use serde_json::Value;
use std::collections::HashMap;
use crate::nostr::wallet_registry::Storage;

pub struct CloudflareStorage {
    storage: worker::Storage,
}

#[async_trait::async_trait(?Send)]
impl Storage for CloudflareStorage {
    async fn get(&self, key: &str) -> Option<Value> {
        self.storage.get(key).await.ok().flatten()
    }
    async fn put_batch(&self, entries: HashMap<String, Value>) {
        for (k, v) in entries {
            let _ = self.storage.put(&k, v).await;
        }
    }
    async fn delete_batch(&self, keys: Vec<String>) {
        for k in keys {
            let _ = self.storage.delete(&k).await;
        }
    }
}

impl CloudflareStorage {
    pub fn new(storage: worker::Storage) -> Self {
        Self { storage }
    }
}
