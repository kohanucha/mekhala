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
        let _ = self.storage.put_multiple(entries).await;
    }
    async fn delete_batch(&self, keys: Vec<String>) {
        let _ = self.storage.delete_multiple(keys).await;
    }
}

impl CloudflareStorage {
    pub fn new(storage: worker::Storage) -> Self {
        Self { storage }
    }
}
