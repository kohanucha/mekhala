use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::cell::Cell;
use async_trait::async_trait;
use serde_json::Value;
use crate::nostr::{Event, Filter, Limits, Tag};
use crate::nostr::wallet_registry::{WalletRegistry, Storage};
use crate::nostr::nip_47::{EncryptionMethod, NwcClient, NwcUri};
use crate::nostr::WalletInfo;
use crate::common::{NwcError, NwcTransport};

// ── Constants ──

pub const TEST_WALLET_SK: &str = "0101010101010101010101010101010101010101010101010101010101010101";
pub const TEST_WALLET_PK: &str = "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f";
pub const TEST_NWC_URI: &str = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";

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
    crate::nostr::engine::NostrEngine::new_with_storage(
        MockStorage::new(),
        Limits::default(),
        test_now,
    )
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
    new_storage.put_batch(snapshot).await.unwrap();
    WalletRegistry::new(new_storage, Limits::default())
}

// ── Storage seed helpers ──

pub async fn seed_subscription(storage: &MockStorage, conn_id: u32, sub_id: &str, pk: &str, filters: Vec<Filter>) {
    let mut entries = HashMap::new();
    entries.insert(format!("pk:{}", pk), serde_json::json!(vec![conn_id]));
    let mut subs = std::collections::HashMap::new();
    subs.insert(sub_id.to_string(), serde_json::to_value(filters).unwrap());
    entries.insert(format!("conn:{}", conn_id), serde_json::json!({
        "subscriptions": subs,
        "info_event": null
    }));
    storage.put_batch(entries).await.unwrap();
}

// ── Consolidated MockTransport ──

pub struct MockTransport {
    pub wallet_info: Option<WalletInfo>,
    pub wallet_uri: Option<NwcUri>,
    pub error_code: Option<String>,
}

impl MockTransport {
    pub fn wallet_not_found() -> Self {
        Self { wallet_info: None, wallet_uri: None, error_code: None }
    }
}

#[async_trait(?Send)]
impl NwcTransport for MockTransport {
    async fn get_wallet_info(&self, _pubkey: &str) -> Option<WalletInfo> {
        self.wallet_info.clone()
    }

    async fn execute_nwc_rpc(&self, request: Event) -> Result<Event, NwcError> {
        let uri = match self.wallet_uri.as_ref() {
            Some(u) => u.clone(),
            None => return Err(NwcError::WalletNotFound),
        };

        let mut wallet_client = NwcClient::new(uri).unwrap();

        let is_nip44 = request.tags.iter().any(|t| t.encryption_scheme() == Some("nip44_v2"));
        if is_nip44 {
            wallet_client.encryption_method = EncryptionMethod::Nip44;
        }

        let _ = wallet_client.decrypt(&request.content).map_err(NwcError::from)?;

        let resp_payload = if let Some(ref code) = self.error_code {
            serde_json::json!({
                "error": {
                    "code": code,
                    "message": "insufficient balance"
                }
            })
        } else {
            serde_json::json!({
                "result": {
                    "invoice": "lnbc1test"
                }
            })
        };

        let encrypted = wallet_client.encrypt(&resp_payload).unwrap();
        let mut tags = vec![
            Tag::p(&wallet_client.my_pubkey),
            Tag::E(request.id.clone(), vec![]),
        ];
        if is_nip44 {
            tags.push(Tag::encryption("nip44_v2"));
        }

        let response_event = wallet_client.create_event(23195, encrypted, tags).unwrap();
        Ok(response_event)
    }
}
