use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use sled::Db;
use tokio_tungstenite::tungstenite::Message;
use futures_util::stream::SplitSink;
use tokio_tungstenite::WebSocketStream;
use tokio::net::TcpStream;

pub type WsSink = SplitSink<WebSocketStream<TcpStream>, Message>;

pub struct WhitelistStore {
    db: Arc<Mutex<Db>>,
}

impl WhitelistStore {
    pub fn new(db: Db) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
        }
    }

    pub async fn add(&self, pubkey: &str) -> Result<bool, String> {
        let db = self.db.lock().await;
        let tree = db
            .open_tree("whitelist")
            .map_err(|e| format!("Failed to open tree: {}", e))?;

        let key = pubkey.as_bytes();
        let was_present = tree.contains_key(key).map_err(|e| format!("DB error: {}", e))?;

        tree.insert(key, vec![])
            .map_err(|e| format!("Failed to insert: {}", e))?;

        Ok(was_present)
    }

    pub async fn remove(&self, pubkey: &str) -> Result<bool, String> {
        let db = self.db.lock().await;
        let tree = db
            .open_tree("whitelist")
            .map_err(|e| format!("Failed to open tree: {}", e))?;

        let key = pubkey.as_bytes();
        let was_present = tree.remove(key).map_err(|e| format!("Failed to remove: {}", e))?;

        Ok(was_present.is_some())
    }

    pub async fn list(&self) -> Result<Vec<String>, String> {
        let db = self.db.lock().await;
        let tree = db
            .open_tree("whitelist")
            .map_err(|e| format!("Failed to open tree: {}", e))?;

        let mut pubkeys = Vec::new();
        for key_result in tree.iter() {
            let (key, _) = key_result.map_err(|e| format!("DB error: {}", e))?;
            if let Ok(pubkey) = String::from_utf8(key.to_vec()) {
                pubkeys.push(pubkey);
            }
        }

        pubkeys.sort();
        Ok(pubkeys)
    }

    pub async fn contains(&self, pubkey: &str) -> Result<bool, String> {
        let db = self.db.lock().await;
        let tree = db
            .open_tree("whitelist")
            .map_err(|e| format!("Failed to open tree: {}", e))?;

        tree.contains_key(pubkey.as_bytes())
            .map_err(|e| format!("DB error: {}", e))
    }
}

pub fn open_whitelist_store(data_dir: &PathBuf) -> Result<WhitelistStore, String> {
    let db = sled::open(data_dir.join("whitelist.db"))
        .map_err(|e| format!("Failed to open DB: {}", e))?;

    Ok(WhitelistStore::new(db))
}