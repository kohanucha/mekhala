use std::collections::{HashMap, HashSet};
use async_trait::async_trait;
use serde_json::Value;
use crate::log_debug;
use crate::log_info;
use crate::log_error;
use crate::nostr::{Filter, Event, Limits};
use crate::util::short;

#[async_trait(?Send)]
pub trait Storage {
    async fn get(&self, key: &str) -> Option<Value>;
    async fn put_batch(&self, entries: HashMap<String, Value>) -> Result<(), String>;
    async fn delete_batch(&self, keys: Vec<String>);
}

pub struct SavedState {
    pub json: Value,
    pub pubkeys: HashSet<String>,
}

/// A purely synchronous index for subscriptions and info events.
type PkEntry = (HashSet<(String, Vec<Filter>)>, Option<Event>);

struct WalletIndex {
    subscription_index: HashMap<(String, Vec<Filter>), Vec<u32>>,
    pk_index: HashMap<String, PkEntry>,
    info_id_index: HashMap<String, String>,
    reverse_index: HashMap<u32, HashMap<String, Vec<Filter>>>,
}

impl WalletIndex {
    fn new() -> Self {
        Self {
            subscription_index: HashMap::new(),
            pk_index: HashMap::new(),
            info_id_index: HashMap::new(),
            reverse_index: HashMap::new(),
        }
    }

    fn subscribe(&mut self, conn_id: u32, sub_id: String, filters: Vec<Filter>) {
        self.unsubscribe(conn_id, sub_id.clone());

        let sub_key = (sub_id.clone(), filters.clone());

        for filter in &filters {
            for pk in filter.pubkeys() {
                let entry = self.pk_index.entry(pk).or_insert_with(|| (HashSet::new(), None));
                entry.0.insert(sub_key.clone());
            }
        }

        let conns = self.subscription_index.entry(sub_key).or_default();
        if !conns.contains(&conn_id) {
            conns.push(conn_id);
        }

        self.reverse_index.entry(conn_id)
            .or_default()
            .insert(sub_id, filters);
    }

    fn unsubscribe(&mut self, conn_id: u32, sub_id: String) {
        if let Some(conn_subs) = self.reverse_index.get_mut(&conn_id) {
            if let Some(filters) = conn_subs.remove(&sub_id) {
                let sub_key = (sub_id, filters);

                if let Some(conns) = self.subscription_index.get_mut(&sub_key) {
                    conns.retain(|&id| id != conn_id);
                    if conns.is_empty() {
                        self.subscription_index.remove(&sub_key);

                        for filter in &sub_key.1 {
                            for pk in filter.pubkeys() {
                                if let Some(entry) = self.pk_index.get_mut(&pk) {
                                    entry.0.remove(&sub_key);
                                    if entry.0.is_empty() && entry.1.is_none() {
                                        self.pk_index.remove(&pk);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if conn_subs.is_empty() {
                self.reverse_index.remove(&conn_id);
            }
        }
    }

    fn disconnect(&mut self, conn_id: u32) {
        let sub_ids: Vec<String> = self.reverse_index.get(&conn_id)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();

        for sub_id in sub_ids {
            self.unsubscribe(conn_id, sub_id);
        }
    }

    fn get_subscriptions(&self, conn_id: u32) -> HashMap<String, Vec<Filter>> {
        self.reverse_index.get(&conn_id).cloned().unwrap_or_default()
    }

    fn sub_count(&self, conn_id: u32) -> usize {
        self.reverse_index.get(&conn_id).map_or(0, |m| m.len())
    }

    fn cache_info(&mut self, event: Event) {
        let entry = self.pk_index.entry(event.pubkey.clone()).or_insert_with(|| (HashSet::new(), None));
        self.info_id_index.insert(event.id.clone(), event.pubkey.clone());
        entry.1 = Some(event);
    }

    fn get_info(&self, pubkey: &str) -> Option<Event> {
        self.pk_index.get(pubkey).and_then(|e| e.1.clone())
    }

    fn delete_info(&mut self, pubkey: &str) -> Option<Event> {
        let removed = if let Some(entry) = self.pk_index.get_mut(pubkey) {
            let old = entry.1.take();
            if let Some(ref event) = old {
                self.info_id_index.remove(&event.id);
            }
            old
        } else {
            None
        };
        // Clean up pk_index entry if both subscriptions and info are gone
        if let Some(entry) = self.pk_index.get(pubkey) {
            if entry.0.is_empty() && entry.1.is_none() {
                self.pk_index.remove(pubkey);
            }
        }
        removed
    }

    fn find_info_pubkey_by_id(&self, event_id: &str) -> Option<String> {
        self.info_id_index.get(event_id).cloned()
    }

    fn match_event<'a>(&'a self, event: &'a Event) -> Box<dyn Iterator<Item = (String, Vec<u32>)> + 'a> {
        let target_pks = event.target_pubkeys();

        let mut sub_to_conns = HashMap::new();
        for pk in target_pks {
            if let Some(entry) = self.pk_index.get(&pk) {
                let mut matching: Vec<(String, u32)> = Vec::new();
                for sub_key in &entry.0 {
                    if sub_key.1.iter().any(|f| f.matches(event)) {
                        if let Some(conns) = self.subscription_index.get(sub_key) {
                            if let Some(&latest_conn) = conns.last() {
                                matching.push((sub_key.0.clone(), latest_conn));
                            }
                        }
                    }
                }
                let latest = matching.iter().map(|(_, id)| *id).max();
                if let Some(latest_id) = latest {
                    for (sub_id, id) in matching {
                        if id == latest_id {
                            sub_to_conns.insert(sub_id, vec![id]);
                        }
                    }
                }
            }
        }
        Box::new(sub_to_conns.into_iter())
    }

    fn get_connection_id(&self, pubkey: &str) -> Option<u32> {
        if let Some(entry) = self.pk_index.get(pubkey) {
            let mut latest = None;
            for sub_key in &entry.0 {
                if let Some(conns) = self.subscription_index.get(sub_key) {
                    if let Some(&id) = conns.last() {
                        match latest {
                            None => latest = Some(id),
                            Some(current) if id > current => latest = Some(id),
                            _ => {}
                        }
                    }
                }
            }
            return latest;
        }
        None
    }

    fn save(&self, conn_id: u32) -> Option<SavedState> {
        let subscriptions = self.get_subscriptions(conn_id);
        if subscriptions.is_empty() {
            return None;
        }

        let mut info_event = None;
        let mut pubkeys = HashSet::new();
        for filters in subscriptions.values() {
            for filter in filters {
                for pk in filter.pubkeys() {
                    pubkeys.insert(pk.clone());
                    if info_event.is_none() {
                        if let Some(event) = self.get_info(&pk) {
                            info_event = Some(event);
                        }
                    }
                }
            }
        }

        Some(SavedState {
            json: serde_json::json!({
                "subscriptions": subscriptions,
                "info_event": info_event,
            }),
            pubkeys,
        })
    }

    fn restore(&mut self, conn_id: u32, data: Value) {
        if let Some(subs_val) = data.get("subscriptions") {
            if let Ok(subs) = serde_json::from_value::<HashMap<String, Vec<Filter>>>(subs_val.clone()) {
                for (sub_id, filters) in subs {
                    self.subscribe(conn_id, sub_id, filters);
                }
            }
        }
        if let Some(info_val) = data.get("info_event") {
            if let Ok(event) = serde_json::from_value::<Event>(info_val.clone()) {
                self.cache_info(event);
            }
        }
    }
}

pub struct WalletRegistry<S: Storage> {
    storage: S,
    index: WalletIndex,
    limits: Limits,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RegistryResponse {
    Send { recipient_id: u32, sub_id: String },
    WakeUp(u32),
}

impl<S: Storage> WalletRegistry<S> {
    pub fn new(storage: S, limits: Limits) -> Self {
        Self { 
            storage,
            index: WalletIndex::new(),
            limits,
        }
    }

    pub async fn subscribe(&mut self, conn_id: u32, sub_id: String, filters: Vec<Filter>) -> crate::nostr::Result<()> {
        let count = self.index.sub_count(conn_id);
        if count >= self.limits.max_subscriptions_per_connection {
            return Err(crate::nostr::RelayError::Generic(format!(
                "too many subscriptions: {} (max {})",
                count, self.limits.max_subscriptions_per_connection
            )));
        }
        self.index.subscribe(conn_id, sub_id, filters);
        self.sync(conn_id).await.map_err(|e| crate::nostr::RelayError::Generic(format!("persist failed: {}", e)))?;
        Ok(())
    }

    pub async fn unsubscribe(&mut self, conn_id: u32, sub_id: String) -> crate::nostr::Result<()> {
        self.index.unsubscribe(conn_id, sub_id.clone());
        if let Err(e) = self.sync(conn_id).await {
            log_error!("unsubscribe sync failed: conn={} sub={} err={}", conn_id, sub_id, e);
        }
        Ok(())
    }

    pub async fn match_event(&mut self, event: &Event) -> Vec<RegistryResponse> {
        let mut responses = Vec::new();
        let target_pks = event.target_pubkeys();

        for pk in target_pks {
            for id in self.load_by_pubkey(&pk).await {
                responses.push(RegistryResponse::WakeUp(id));
            }
        }

        for (sub_id, conns) in self.index.match_event(event) {
            for id in conns {
                responses.push(RegistryResponse::Send { recipient_id: id, sub_id: sub_id.clone() });
            }
        }
        responses
    }

    pub async fn load(&mut self, conn_id: u32) -> bool {
        if !self.index.get_subscriptions(conn_id).is_empty() {
            return true;
        }
        let key = format!("conn:{}", conn_id);
        if let Some(data) = self.storage.get(&key).await {
            self.index.restore(conn_id, data);
            let count = self.index.get_subscriptions(conn_id).len();
            log_info!("restored conn={} with {} subscriptions", conn_id, count);
            return true;
        }
        log_debug!("load: conn={} not found in storage", conn_id);
        false
    }

    pub async fn load_by_pubkey(&mut self, pubkey: &str) -> Vec<u32> {
        let key = format!("pk:{}", pubkey);
        let storage_ids: Vec<u32> = if let Some(val) = self.storage.get(&key).await {
            match &val {
                Value::Array(arr) => arr.iter().filter_map(|v| v.as_u64().map(|x| x as u32)).collect(),
                Value::Number(n) => n.as_u64().map(|x| vec![x as u32]).unwrap_or_default(),
                _ => Vec::new(),
            }
        } else {
            if let Some(id) = self.index.get_connection_id(pubkey) {
                return vec![id];
            }
            return Vec::new();
        };
        let mut loaded = Vec::new();
        let mut stale = Vec::new();
        for id in storage_ids {
            if self.load(id).await {
                loaded.push(id);
            } else {
                stale.push(id);
            }
        }
        if loaded.is_empty() {
            log_debug!("stale pubkey index cleaned: pk={}", short(pubkey, 8));
            self.storage.delete_batch(vec![key]).await;
        } else if !stale.is_empty() {
            let mut entries = HashMap::new();
            entries.insert(key, serde_json::json!(loaded));
            let _ = self.storage.put_batch(entries).await;
        }
        loaded
    }

    pub async fn on_disconnect(&mut self, id: u32) {
        self.index.disconnect(id);
    }

    pub async fn on_terminate(&mut self, id: u32) {
        let subs = self.index.get_subscriptions(id);
        let mut pubkeys: HashSet<String> = HashSet::new();
        for filters in subs.values() {
            for filter in filters {
                for pk in filter.pubkeys() {
                    pubkeys.insert(pk);
                }
            }
        }

        self.index.disconnect(id);

        for pk in &pubkeys {
            let key = format!("pk:{}", pk);
            if let Some(val) = self.storage.get(&key).await {
                let ids: Vec<u32> = match &val {
                    Value::Array(arr) => arr.iter().filter_map(|v| v.as_u64().map(|x| x as u32)).collect(),
                    Value::Number(n) => n.as_u64().map(|x| vec![x as u32]).unwrap_or_default(),
                    _ => Vec::new(),
                };
                let new_ids: Vec<u32> = ids.into_iter().filter(|x| *x != id).collect();
                if new_ids.is_empty() {
                    self.storage.delete_batch(vec![key]).await;
                } else {
                    let mut entries = HashMap::new();
                    entries.insert(key, serde_json::json!(new_ids));
                    let _ = self.storage.put_batch(entries).await;
                }
            }
        }

        log_debug!("deleted conn state: conn={}", id);
        self.storage.delete_batch(vec![format!("conn:{}", id)]).await;
    }

    pub async fn cache_info(&mut self, event: Event) {
        log_debug!("persist info: pk={}", short(&event.pubkey, 8));
        let key = format!("info:{}", event.pubkey);
        let value = serde_json::to_value(&event).unwrap_or(serde_json::Value::Null);
        if !value.is_null() {
            let mut entries = HashMap::new();
            entries.insert(key, value);
            let _ = self.storage.put_batch(entries).await;
        }
        self.index.cache_info(event);
    }

    pub async fn get_info(&mut self, pubkey: &str) -> Option<Event> {
        if let Some(event) = self.index.get_info(pubkey) {
            return Some(event);
        }

        let key = format!("info:{}", pubkey);
        if let Some(val) = self.storage.get(&key).await {
            if let Ok(event) = serde_json::from_value::<Event>(val) {
                log_debug!("info restored from storage: pk={}", short(pubkey, 8));
                self.index.cache_info(event.clone());
                return Some(event);
            }
        }
        None
    }

    pub async fn delete_info(&mut self, pubkey: &str) {
        let removed = self.index.delete_info(pubkey);
        if removed.is_some() {
            log_debug!("info deleted: pk={}", short(pubkey, 8));
            let key = format!("info:{}", pubkey);
            self.storage.delete_batch(vec![key]).await;
        }
    }

    pub fn find_info_pubkey_by_id(&self, event_id: &str) -> Option<String> {
        self.index.find_info_pubkey_by_id(event_id)
    }

    async fn sync(&self, conn_id: u32) -> Result<(), String> {
        if let Some(state) = self.index.save(conn_id) {
            let mut entries = HashMap::new();
            entries.insert(format!("conn:{}", conn_id), state.json);
            for pk in state.pubkeys {
                let key = format!("pk:{}", pk);
                let mut ids = self.read_pk_list(&key).await;
                if !ids.contains(&conn_id) {
                    ids.push(conn_id);
                }
                entries.insert(key, serde_json::json!(ids));
            }
            self.storage.put_batch(entries).await?;
        } else {
            self.storage.delete_batch(vec![format!("conn:{}", conn_id)]).await;
        }
        Ok(())
    }

    async fn read_pk_list(&self, key: &str) -> Vec<u32> {
        if let Some(val) = self.storage.get(key).await {
            match &val {
                Value::Array(arr) => arr.iter().filter_map(|v| v.as_u64().map(|x| x as u32)).collect(),
                Value::Number(n) => n.as_u64().map(|x| vec![x as u32]).unwrap_or_default(),
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
#[path = "wallet_registry_test.rs"]
mod wallet_registry_test;
