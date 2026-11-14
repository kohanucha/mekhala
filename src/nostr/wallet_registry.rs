use crate::log_debug;
use crate::log_error;
use crate::log_info;
use crate::nostr::{Event, Filter, Limits, StorageError};
use crate::util::short;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Persistence boundary of the registry: a key/value store backed by Durable
/// Object storage in production (see `cloudflare`) and `MockStorage` in
/// tests. Keys follow the `conn:<id>`, `pk:<pubkey>`, `eid:<event id>`,
/// `etag:<event id>`, `info:<pubkey>`, `infoid:<event id>` and
/// `info-p:<pubkey>` conventions.
#[async_trait(?Send)]
pub trait Storage {
    /// Reads the JSON value stored under `key`, if any.
    async fn get(&self, key: &str) -> Option<Value>;
    /// Writes (or overwrites) all entries in one batch; `Err` carries the
    /// backend failure as a [`StorageError`].
    async fn put_batch(&self, entries: HashMap<String, Value>) -> Result<(), StorageError>;
    /// Removes the given keys; deletions are best-effort (no error path).
    async fn delete_batch(&self, keys: Vec<String>);
}

/// Snapshot of one connection's state, produced by the internal index and
/// persisted under the `conn:<id>` key: the JSON to store (subscriptions)
/// and the routing keys referenced by that connection's filters.
///
/// The cached wallet info event is deliberately *not* part of this snapshot:
/// `info:<pubkey>` is the single source of truth, so a hibernated connection
/// waking up can never resurrect an info event that the wallet has since
/// replaced or deleted.
pub struct SavedState {
    /// JSON to persist for this connection.
    pub json: Value,
    /// Public keys referenced by the connection's filters (`authors`, `#p`).
    pub pubkeys: HashSet<String>,
    /// Event IDs referenced by the connection's `ids` filters.
    pub event_ids: HashSet<String>,
    /// Event IDs referenced by the connection's `#e` filters.
    pub e_tags: HashSet<String>,
}

/// A purely synchronous index for subscriptions and info events.
type SubKey = (String, Vec<Filter>);
type PkEntry = (HashSet<SubKey>, Option<Event>);

struct WalletIndex {
    subscription_index: HashMap<SubKey, Vec<u32>>,
    pk_index: HashMap<String, PkEntry>,
    id_index: HashMap<String, HashSet<SubKey>>,
    e_index: HashMap<String, HashSet<SubKey>>,
    info_id_index: HashMap<String, String>,
    info_p_index: HashMap<String, String>,
    reverse_index: HashMap<u32, HashMap<String, Vec<Filter>>>,
}

/// Public keys an info event is addressed to through its `p` tags.
fn p_tag_pubkeys(event: &Event) -> Vec<String> {
    event
        .tags
        .iter()
        .filter_map(|t| t.pubkey().map(str::to_string))
        .collect()
}

/// Persisted routing keys (`pk:`, `eid:`, `etag:`) a connection's
/// subscriptions reference. `sync()` adds the connection to these keys, so
/// removing subscriptions has to take it out again.
fn referenced_keys(subs: &HashMap<String, Vec<Filter>>) -> HashSet<String> {
    let mut keys = HashSet::new();
    for filters in subs.values() {
        for filter in filters {
            for pk in filter.pubkeys() {
                keys.insert(format!("pk:{pk}"));
            }
            for event_id in filter.ids.iter().flatten() {
                keys.insert(format!("eid:{event_id}"));
            }
            for event_id in filter.e_tags.iter().flatten() {
                keys.insert(format!("etag:{event_id}"));
            }
        }
    }
    keys
}

/// Removes a subscription from one of the secondary indexes, dropping the
/// key entirely once no subscription references it.
fn remove_from_index(index: &mut HashMap<String, HashSet<SubKey>>, key: &str, sub_key: &SubKey) {
    let empty = match index.get_mut(key) {
        Some(subs) => {
            subs.remove(sub_key);
            subs.is_empty()
        }
        None => false,
    };
    if empty {
        index.remove(key);
    }
}

impl WalletIndex {
    fn new() -> Self {
        Self {
            subscription_index: HashMap::new(),
            pk_index: HashMap::new(),
            id_index: HashMap::new(),
            e_index: HashMap::new(),
            info_id_index: HashMap::new(),
            info_p_index: HashMap::new(),
            reverse_index: HashMap::new(),
        }
    }

    fn subscribe(&mut self, conn_id: u32, sub_id: String, filters: Vec<Filter>) {
        self.unsubscribe(conn_id, sub_id.clone());

        let sub_key = (sub_id.clone(), filters.clone());

        for filter in &filters {
            for pk in filter.pubkeys() {
                let entry = self
                    .pk_index
                    .entry(pk)
                    .or_insert_with(|| (HashSet::new(), None));
                entry.0.insert(sub_key.clone());
            }
            for id in filter.ids.iter().flatten() {
                self.id_index
                    .entry(id.clone())
                    .or_default()
                    .insert(sub_key.clone());
            }
            for event_id in filter.e_tags.iter().flatten() {
                self.e_index
                    .entry(event_id.clone())
                    .or_default()
                    .insert(sub_key.clone());
            }
        }

        let conns = self.subscription_index.entry(sub_key).or_default();
        if !conns.contains(&conn_id) {
            conns.push(conn_id);
        }

        self.reverse_index
            .entry(conn_id)
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
                            for id in filter.ids.iter().flatten() {
                                remove_from_index(&mut self.id_index, id, &sub_key);
                            }
                            for event_id in filter.e_tags.iter().flatten() {
                                remove_from_index(&mut self.e_index, event_id, &sub_key);
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
        let sub_ids: Vec<String> = self
            .reverse_index
            .get(&conn_id)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();

        for sub_id in sub_ids {
            self.unsubscribe(conn_id, sub_id);
        }
    }

    fn get_subscriptions(&self, conn_id: u32) -> HashMap<String, Vec<Filter>> {
        self.reverse_index
            .get(&conn_id)
            .cloned()
            .unwrap_or_default()
    }

    fn sub_count(&self, conn_id: u32) -> usize {
        self.reverse_index.get(&conn_id).map_or(0, |m| m.len())
    }

    fn has_subscription(&self, conn_id: u32, sub_id: &str) -> bool {
        self.reverse_index
            .get(&conn_id)
            .is_some_and(|m| m.contains_key(sub_id))
    }

    fn cache_info(&mut self, event: Event) {
        // The author has at most one info event: drop the reverse id mapping
        // of the previous one before recording the new event.
        if let Some(previous_id) = self
            .pk_index
            .get(&event.pubkey)
            .and_then(|entry| entry.1.as_ref())
            .map(|previous| previous.id.clone())
        {
            self.info_id_index.remove(&previous_id);
        }

        // Drop aliases that belonged to the previous info event of this
        // author and are not part of the new one (a wallet may re-publish
        // with a different `p` tag set).
        let stale_aliases = self.stale_info_aliases(&event);
        for pk in stale_aliases {
            self.info_p_index.remove(&pk);
        }

        let entry = self
            .pk_index
            .entry(event.pubkey.clone())
            .or_insert_with(|| (HashSet::new(), None));
        self.info_id_index
            .insert(event.id.clone(), event.pubkey.clone());
        for pk in p_tag_pubkeys(&event) {
            self.info_p_index.insert(pk, event.pubkey.clone());
        }
        entry.1 = Some(event);
    }

    /// Aliases of the author's previous info event that the new event no
    /// longer carries.
    fn stale_info_aliases(&self, event: &Event) -> Vec<String> {
        let current: HashSet<String> = p_tag_pubkeys(event).into_iter().collect();
        self.pk_index
            .get(&event.pubkey)
            .and_then(|entry| entry.1.as_ref())
            .map(|previous| {
                p_tag_pubkeys(previous)
                    .into_iter()
                    .filter(|pk| !current.contains(pk))
                    .filter(|pk| {
                        self.info_p_index
                            .get(pk)
                            .is_some_and(|author| author == &event.pubkey)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Latest info event addressed to `pubkey`, either as its author or as a
    /// `p` tag target (NWA apps learn about a wallet through the `p` tag).
    fn get_info(&self, pubkey: &str) -> Option<Event> {
        if let Some(event) = self.pk_index.get(pubkey).and_then(|e| e.1.clone()) {
            return Some(event);
        }
        let author = self.info_p_index.get(pubkey)?;
        self.pk_index.get(author).and_then(|e| e.1.clone())
    }

    /// Removes the cached info event authored by `pubkey` and returns it.
    fn delete_info(&mut self, pubkey: &str) -> Option<Event> {
        let removed = self
            .pk_index
            .get_mut(pubkey)
            .and_then(|entry| entry.1.take());
        if let Some(ref event) = removed {
            self.info_id_index.remove(&event.id);
        }
        // Aliases are keyed by target and two wallets can address the same
        // app, so only the aliases owned by this author are dropped.
        self.info_p_index.retain(|_, owner| owner != pubkey);
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

    /// Adds every connection registered for a matching subscription to `out`,
    /// keyed by subscription id.
    ///
    /// A reconnecting client leaves its previous id behind until its old
    /// socket reports the close, and each recorded id belongs to a real
    /// subscription — so all of them receive the event. Delivering to the
    /// newest id only (as this used to) let any later connection, including
    /// an attacker's, silence every other subscriber of the same wallet.
    fn collect_matches(
        &self,
        subs: &HashSet<SubKey>,
        event: &Event,
        out: &mut HashMap<String, Vec<u32>>,
    ) {
        for sub_key in subs {
            if !sub_key.1.iter().any(|f| f.matches(event)) {
                continue;
            }
            let Some(conns) = self.subscription_index.get(sub_key) else {
                continue;
            };
            for id in conns {
                let recipients = out.entry(sub_key.0.clone()).or_default();
                if !recipients.contains(id) {
                    recipients.push(*id);
                }
            }
        }
    }

    fn match_event(&self, event: &Event) -> impl Iterator<Item = (String, Vec<u32>)> {
        let mut sub_to_conns = HashMap::new();

        for pk in event.target_pubkeys() {
            if let Some(entry) = self.pk_index.get(&pk) {
                self.collect_matches(&entry.0, event, &mut sub_to_conns);
            }
        }
        if let Some(subs) = self.id_index.get(&event.id) {
            self.collect_matches(subs, event, &mut sub_to_conns);
        }
        for event_id in event.tags.iter().filter_map(|t| t.event_id()) {
            if let Some(subs) = self.e_index.get(event_id) {
                self.collect_matches(subs, event, &mut sub_to_conns);
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

        let mut pubkeys = HashSet::new();
        let mut event_ids = HashSet::new();
        let mut e_tags = HashSet::new();
        for filters in subscriptions.values() {
            for filter in filters {
                for pk in filter.pubkeys() {
                    pubkeys.insert(pk);
                }
                if let Some(ids) = &filter.ids {
                    event_ids.extend(ids.iter().cloned());
                }
                if let Some(ids) = &filter.e_tags {
                    e_tags.extend(ids.iter().cloned());
                }
            }
        }

        Some(SavedState {
            json: serde_json::json!({
                "subscriptions": subscriptions,
            }),
            pubkeys,
            event_ids,
            e_tags,
        })
    }

    fn restore(&mut self, conn_id: u32, data: &Value) {
        if let Some(subs_val) = data.get("subscriptions") {
            if let Ok(subs) =
                serde_json::from_value::<HashMap<String, Vec<Filter>>>(subs_val.clone())
            {
                for (sub_id, filters) in subs {
                    self.subscribe(conn_id, sub_id, filters);
                }
            }
        }
        // Older snapshots may still contain an `info_event` copy; it is
        // ignored on purpose. Storage (`info:<pubkey>`) is authoritative.
    }
}

/// Central connection registry: tracks which connection subscribes to which
/// filters, persists per-connection state through [`Storage`] so
/// subscriptions survive Durable Object hibernation, and caches the
/// wallet-service info events needed to answer subscription requests.
pub struct WalletRegistry<S: Storage> {
    storage: S,
    index: WalletIndex,
    limits: Limits,
}

/// An action the transport must take after [`WalletRegistry::match_event`].
#[derive(Debug, PartialEq, Eq)]
pub enum RegistryResponse {
    /// Deliver the event on the given subscription of this live connection.
    Send { recipient_id: u32, sub_id: String },
    /// The event matches this connection via the persisted pubkey index,
    /// but its live subscription state is not loaded (e.g. its WebSocket is
    /// hibernating) — the transport must hydrate the connection (reload
    /// state) before delivery.
    WakeUp(u32),
}

impl<S: Storage> WalletRegistry<S> {
    /// Creates a registry over the given storage with relay limits applied
    /// (max subscriptions per connection).
    pub fn new(storage: S, limits: Limits) -> Self {
        Self {
            storage,
            index: WalletIndex::new(),
            limits,
        }
    }

    /// Registers a subscription for the connection and persists it to
    /// storage (the hibernation contract: without persistence the
    /// subscription is lost when the DO sleeps).
    ///
    /// # Errors
    ///
    /// Returns [`RelayError::Generic`](crate::nostr::RelayError::Generic)
    /// when a *new* subscription would exceed
    /// `limits.max_subscriptions_per_connection` or
    /// `limits.max_routing_keys_per_connection`, or when persistence fails. A
    /// `REQ` that reuses an existing `sub_id` replaces that subscription
    /// (NIP-01), so it is allowed at the limit.
    pub async fn subscribe(
        &mut self,
        conn_id: u32,
        sub_id: String,
        filters: Vec<Filter>,
    ) -> crate::nostr::Result<()> {
        let count = self.index.sub_count(conn_id);
        let replacing = self.index.has_subscription(conn_id, &sub_id);
        if !replacing && count >= self.limits.max_subscriptions_per_connection {
            return Err(crate::nostr::RelayError::Generic(format!(
                "too many subscriptions: {} (max {})",
                count, self.limits.max_subscriptions_per_connection
            )));
        }

        // `sync()` reads and rewrites every routing key of the connection, so
        // the budget is checked against the whole connection, not this filter.
        let mut prospective = self.index.get_subscriptions(conn_id);
        prospective.insert(sub_id.clone(), filters.clone());
        let keys = referenced_keys(&prospective).len();
        if keys > self.limits.max_routing_keys_per_connection {
            return Err(crate::nostr::RelayError::Generic(format!(
                "too many routing keys: {keys} (max {})",
                self.limits.max_routing_keys_per_connection
            )));
        }

        self.index.subscribe(conn_id, sub_id, filters);
        self.sync(conn_id)
            .await
            .map_err(|e| crate::nostr::RelayError::Generic(format!("persist failed: {e}")))?;
        Ok(())
    }

    /// Removes a subscription and re-persists the connection state; never
    /// fails (persistence problems are logged).
    ///
    /// Also drops the connection from any `pk:`/`eid:`/`etag:` key that no
    /// remaining subscription references.
    pub async fn unsubscribe(&mut self, conn_id: u32, sub_id: String) -> crate::nostr::Result<()> {
        let before = referenced_keys(&self.index.get_subscriptions(conn_id));
        self.index.unsubscribe(conn_id, sub_id.clone());
        let after = referenced_keys(&self.index.get_subscriptions(conn_id));
        for key in before.difference(&after) {
            self.remove_conn_from_index(key.clone(), conn_id).await;
        }
        if let Err(e) = self.sync(conn_id).await {
            log_error!(
                "unsubscribe sync failed: conn={} sub={} err={}",
                conn_id,
                sub_id,
                e
            );
        }
        Ok(())
    }

    /// Matches an event against every live subscription plus the persisted
    /// routing indexes (pubkey, event ID, `#e`), returning the delivery
    /// actions for the transport (see [`RegistryResponse`]). Loading
    /// connections from storage may also prune stale index entries.
    pub async fn match_event(&mut self, event: &Event) -> Vec<RegistryResponse> {
        let mut responses = Vec::new();
        let mut woken = HashSet::new();

        let mut keys: Vec<String> = event
            .target_pubkeys()
            .into_iter()
            .map(|pk| format!("pk:{pk}"))
            .collect();
        keys.push(format!("eid:{}", event.id));
        for event_id in event.tags.iter().filter_map(|t| t.event_id()) {
            keys.push(format!("etag:{event_id}"));
        }

        for key in keys {
            for id in self.load_indexed(&key).await.unwrap_or_default() {
                if woken.insert(id) {
                    responses.push(RegistryResponse::WakeUp(id));
                }
            }
        }

        for (sub_id, conns) in self.index.match_event(event) {
            for id in conns {
                responses.push(RegistryResponse::Send {
                    recipient_id: id,
                    sub_id: sub_id.clone(),
                });
            }
        }
        responses
    }

    /// Restores the connection's subscriptions from storage into the live
    /// index; returns whether it has subscriptions afterwards. The cached
    /// info event is read lazily from `info:<pubkey>`, never from the
    /// connection snapshot.
    pub async fn load(&mut self, conn_id: u32) -> bool {
        if !self.index.get_subscriptions(conn_id).is_empty() {
            return true;
        }
        let key = format!("conn:{conn_id}");
        if let Some(data) = self.storage.get(&key).await {
            self.index.restore(conn_id, &data);
            let count = self.index.get_subscriptions(conn_id).len();
            log_info!("restored conn={} with {} subscriptions", conn_id, count);
            return true;
        }
        log_debug!("load: conn={} not found in storage", conn_id);
        false
    }

    /// Connection IDs to notify about events addressed to `pubkey`: reads
    /// the persisted `pk:<pubkey>` index and loads each connection, pruning
    /// stale IDs from storage. When nothing is persisted, falls back to the
    /// single live connection mapped in memory.
    pub async fn load_by_pubkey(&mut self, pubkey: &str) -> Vec<u32> {
        match self.load_indexed(&format!("pk:{pubkey}")).await {
            Some(loaded) => loaded,
            None => self.index.get_connection_id(pubkey).into_iter().collect(),
        }
    }

    /// Loads every connection listed under a persisted routing key
    /// (`pk:`, `eid:`, `etag:`), pruning entries whose connection state is
    /// gone. Returns `None` when the key does not exist in storage.
    async fn load_indexed(&mut self, key: &str) -> Option<Vec<u32>> {
        let storage_ids = parse_conn_ids(&self.storage.get(key).await?);

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
            log_debug!("stale index cleaned: key={}", short(key, 24));
            self.storage.delete_batch(vec![key.to_string()]).await;
        } else if !stale.is_empty() {
            let mut entries = HashMap::new();
            entries.insert(key.to_string(), serde_json::json!(loaded));
            let _ = self.storage.put_batch(entries).await;
        }
        Some(loaded)
    }

    /// Final teardown: drops live index entries and deletes persisted state
    /// (`conn:<id>` plus its `pk:`/`eid:`/`etag:` memberships).
    pub async fn on_terminate(&mut self, id: u32) {
        let keys = referenced_keys(&self.index.get_subscriptions(id));

        self.index.disconnect(id);

        for key in keys {
            self.remove_conn_from_index(key, id).await;
        }

        log_debug!("deleted conn state: conn={}", id);
        self.storage
            .delete_batch(vec![format!("conn:{}", id)])
            .await;
    }

    /// Caches a wallet-service info event in memory and in storage:
    /// `info:<author>` holds the event itself, `infoid:<event id>` maps the
    /// event back to its author (so kind `5` deletion by `e` tag still works
    /// after hibernation), and `info-p:<p tag>` maps each addressed app back
    /// to the author so `#p`-scoped subscriptions can be replayed.
    pub async fn cache_info(&mut self, event: Event) {
        log_debug!("persist info: pk={}", short(&event.pubkey, 8));

        // The author has at most one info event. A republish must drop the
        // previous event's reverse mapping and any `p` alias it no longer
        // carries; after hibernation only storage knows the previous event.
        let previous = match self
            .index
            .pk_index
            .get(&event.pubkey)
            .and_then(|entry| entry.1.clone())
        {
            Some(previous) => Some(previous),
            None => {
                self.read_info_event(&format!("info:{}", event.pubkey))
                    .await
            }
        };

        let mut stale_keys: Vec<String> = Vec::new();
        if let Some(previous) = &previous {
            let current: HashSet<String> = p_tag_pubkeys(&event).into_iter().collect();
            for pk in p_tag_pubkeys(previous) {
                if current.contains(&pk) {
                    continue;
                }
                let alias_key = format!("info-p:{pk}");
                let owner = self.storage.get(&alias_key).await;
                if owner.as_ref().and_then(Value::as_str) == Some(event.pubkey.as_str()) {
                    stale_keys.push(alias_key);
                }
            }
            if previous.id != event.id {
                stale_keys.push(format!("infoid:{}", previous.id));
            }
        }
        if !stale_keys.is_empty() {
            self.storage.delete_batch(stale_keys).await;
        }

        let mut entries = HashMap::new();
        let value = serde_json::to_value(&event).unwrap_or(serde_json::Value::Null);
        if !value.is_null() {
            entries.insert(format!("info:{}", event.pubkey), value);
        }
        entries.insert(
            format!("infoid:{}", event.id),
            serde_json::json!(event.pubkey.clone()),
        );
        for pk in p_tag_pubkeys(&event) {
            entries.insert(
                format!("info-p:{pk}"),
                serde_json::json!(event.pubkey.clone()),
            );
        }
        if !entries.is_empty() {
            let _ = self.storage.put_batch(entries).await;
        }
        self.index.cache_info(event);
    }

    /// Latest wallet-service info event addressed to `pubkey`: either the
    /// event's author or one of its `p` tag targets. Reads the in-memory
    /// index first, then `info:<pubkey>`, then the `info-p:<pubkey>` alias.
    pub async fn get_info(&mut self, pubkey: &str) -> Option<Event> {
        if let Some(event) = self.index.get_info(pubkey) {
            return Some(event);
        }

        if let Some(event) = self.read_info_event(&format!("info:{pubkey}")).await {
            log_debug!("info restored from storage: pk={}", short(pubkey, 8));
            self.index.cache_info(event.clone());
            return Some(event);
        }

        let author = self
            .storage
            .get(&format!("info-p:{pubkey}"))
            .await?
            .as_str()?
            .to_string();
        let event = self.read_info_event(&format!("info:{author}")).await?;
        log_debug!(
            "info restored from p-tag alias: pk={} author={}",
            short(pubkey, 8),
            short(&author, 8)
        );
        self.index.cache_info(event.clone());
        Some(event)
    }

    async fn read_info_event(&self, key: &str) -> Option<Event> {
        let val = self.storage.get(key).await?;
        serde_json::from_value::<Event>(val).ok()
    }

    /// Removes the cached info event for `pubkey` (its author) from memory
    /// and storage, including every `p` tag alias that resolves to it and
    /// the event's `infoid:` reverse mapping. Aliases owned by another author
    /// (two wallets can address the same app) are left untouched, and the
    /// event is read from storage when the in-memory index was lost to
    /// hibernation.
    pub async fn delete_info(&mut self, pubkey: &str) {
        let removed = self.index.delete_info(pubkey);

        let stored_event = match removed {
            Some(event) => Some(event),
            None => self.read_info_event(&format!("info:{pubkey}")).await,
        };

        let mut keys = vec![format!("info:{pubkey}")];
        match stored_event {
            Some(event) => {
                log_debug!("info deleted: pk={}", short(pubkey, 8));
                keys.push(format!("infoid:{}", event.id));
                for pk in p_tag_pubkeys(&event) {
                    let alias_key = format!("info-p:{pk}");
                    let owner = self.storage.get(&alias_key).await;
                    if owner.as_ref().and_then(Value::as_str) == Some(pubkey) {
                        keys.push(alias_key);
                    }
                }
            }
            None => {
                log_debug!("info delete: nothing cached for pk={}", short(pubkey, 8));
            }
        }
        self.storage.delete_batch(keys).await;
    }

    /// Maps an info event's ID back to the wallet service's public key.
    /// Reads the in-memory index first and falls back to the persisted
    /// `infoid:` key, so the lookup works after hibernation.
    pub async fn find_info_pubkey_by_id(&self, event_id: &str) -> Option<String> {
        if let Some(pubkey) = self.index.find_info_pubkey_by_id(event_id) {
            return Some(pubkey);
        }
        self.storage
            .get(&format!("infoid:{event_id}"))
            .await?
            .as_str()
            .map(str::to_string)
    }

    /// Handles a kind `5` deletion that references an info event by id: the
    /// event is deleted only when `author` signed the deletion and the id is
    /// still the author's current info event. A superseded id (the author
    /// republished since) is ignored so an old reference cannot delete the
    /// newer event. Returns whether something was deleted.
    pub async fn delete_info_by_event(&mut self, event_id: &str, author: &str) -> bool {
        let Some(info_pk) = self.find_info_pubkey_by_id(event_id).await else {
            return false;
        };
        if info_pk != author {
            log_debug!("info delete by e-tag denied: author mismatch");
            return false;
        }
        match self.get_info(&info_pk).await {
            Some(event) if event.id == event_id => {
                self.delete_info(&info_pk).await;
                true
            }
            Some(_) => {
                log_debug!("info delete by e-tag ignored: superseded event id");
                false
            }
            None => false,
        }
    }

    async fn sync(&self, conn_id: u32) -> Result<(), StorageError> {
        if let Some(state) = self.index.save(conn_id) {
            let mut entries = HashMap::new();
            entries.insert(format!("conn:{conn_id}"), state.json);
            for pk in state.pubkeys {
                self.append_conn(&mut entries, format!("pk:{pk}"), conn_id)
                    .await;
            }
            for event_id in state.event_ids {
                self.append_conn(&mut entries, format!("eid:{event_id}"), conn_id)
                    .await;
            }
            for event_id in state.e_tags {
                self.append_conn(&mut entries, format!("etag:{event_id}"), conn_id)
                    .await;
            }
            self.storage.put_batch(entries).await?;
        } else {
            self.storage
                .delete_batch(vec![format!("conn:{}", conn_id)])
                .await;
        }
        Ok(())
    }

    /// Adds `conn_id` to the persisted connection list under `key`.
    async fn append_conn(&self, entries: &mut HashMap<String, Value>, key: String, conn_id: u32) {
        let mut ids = parse_conn_ids(&self.storage.get(&key).await.unwrap_or(Value::Null));
        if !ids.contains(&conn_id) {
            ids.push(conn_id);
        }
        entries.insert(key, serde_json::json!(ids));
    }

    /// Removes `conn_id` from the persisted connection list under `key`.
    async fn remove_conn_from_index(&self, key: String, conn_id: u32) {
        if let Some(val) = self.storage.get(&key).await {
            let ids = parse_conn_ids(&val);
            let new_ids: Vec<u32> = ids.into_iter().filter(|x| *x != conn_id).collect();
            if new_ids.is_empty() {
                self.storage.delete_batch(vec![key]).await;
            } else {
                let mut entries = HashMap::new();
                entries.insert(key, serde_json::json!(new_ids));
                let _ = self.storage.put_batch(entries).await;
            }
        }
    }
}

/// Decodes a persisted connection list. Accepts both an array and a single
/// number for state written by older versions.
fn parse_conn_ids(value: &Value) -> Vec<u32> {
    match value {
        Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_u64().map(|x| x as u32))
            .collect(),
        Value::Number(n) => n.as_u64().map(|x| vec![x as u32]).unwrap_or_default(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
#[path = "wallet_registry_test.rs"]
mod wallet_registry_test;
