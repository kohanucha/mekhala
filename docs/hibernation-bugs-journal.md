# WebSocket Hibernation API — Bug Journal

All bugs encountered during migration from custom `HibernationState` JS interop
to Cloudflare's native WebSocket Hibernation API (`serialize_attachment`).

---

## Bug 1: `accept_websocket_with_tags` silently breaks message routing

**Date:** 2026-05, PR #9  
**Severity:** Critical (production-dead)

### Symptom
- `websocket_message` handler never invoked in production
- Tags persisted correctly (`get_tags` confirmed)
- `wrangler dev` integration tests passed — messages routed fine locally
- Connections were accepted but received no events

### Root Cause
Replaced custom `HibernationState` JS interop with `accept_websocket_with_tags(ws, tags)`.
While tags were stored, Cloudflare's production runtime did not route incoming
WebSocket messages to the `websocket_message` handler. The exact mechanism is
unclear (likely a bug in the runtime's tag-based routing at the time), but the
result was consistent: zero `websocket_message` invocations.

### Lesson
- `accept_websocket_with_tags` cannot be relied upon for message routing in
  the Cloudflare Workers WebSocket Hibernation API.
- Use `accept_web_socket(ws)` + `serialize_attachment(&data)` instead.

---

## Bug 2: `serialize_attachment` BEFORE vs AFTER `accept_web_socket` — **order matters**

**Date:** 2026-05-20, PR #11 (broken) / PR #12 (fix)  
**Severity:** Critical (production-dead)

### Symptom
- Connections registered: `✓ conn=N registered, attachment verified`
- `websocket_message` handler never invoked in production
- `wrangler dev` passed — messages routed fine locally
- Alby Go timed out with no `← RECV` logs

### Root Cause
Calling `serialize_attachment(&id)` **after** `accept_web_socket(ws)`:

```rust
// BROKEN — PR #11:
state.accept_web_socket(ws);       // 1. Accept FIRST
ws.serialize_attachment(&id);       // 2. THEN set attachment

// WORKING — commit 1959c4c:
server.serialize_attachment(&id)?;  // 1. Serialize FIRST
self.state.accept_web_socket(&server); // 2. THEN accept
```

In Cloudflare's production runtime, setting the attachment **after** acceptance
corrupts the WebSocket's Hibernation state, preventing the runtime from routing
incoming messages to `websocket_message`. The attachment data IS stored, but
message routing silently breaks.

### Why integration tests passed locally
`wrangler dev` (miniflare/workerd) does not exercise the Hibernation API the
same way as production. Locally, WebSocket events are routed through an
internal event loop regardless of accept/attachment order. Only production
enforces the full Hibernation API contract.

### Lesson
- **`serialize_attachment` MUST be called before `accept_web_socket`.**
  Always. No exceptions.
- `wrangler dev` **cannot catch Hibernation API ordering bugs**. Any code
  that touches attachment serialization or WebSocket acceptance must be
  tested in production on Cloudflare's infrastructure.

---

## Bug 3: `pk:{pubkey}` storage overwrite — only one connection per pubkey

**Date:** 2026-05-20, PR #13  
**Severity:** Critical (production — events not routed after hibernation)

### Symptom
- `websocket_message` fired (Bug 2 fixed)
- `event kind=23194 → 0 subscribers` despite wallet having an active subscription
- Wallet (conn=5076) subscribed `kinds=[23194] #p=c231a264`
- App (conn=8076) subscribed `kinds=[23195] authors=c231a264`
- After hibernation, only app's subscription data restored
- Wallet's subscription undiscoverable → events silently dropped

### Root Cause
`sync()` stored `pk:{pubkey}` as a single `conn_id` (JSON number):

```rust
// OLD — overwrites previous connection's entry:
for pk in state.pubkeys {
    entries.insert(format!("pk:{}", pk), serde_json::json!(conn_id));
}
```

When two connections subscribe with the same pubkey:
1. Wallet writes `pk:c231a264 → 1779235076`
2. App writes `pk:c231a264 → 1779238076` (overwrites wallet's entry!)

After DO hibernation, `load_by_pubkey("c231a264")` only finds conn=1779238076
(the app), loads app's `kinds=[23195]` subscription, but the wallet's
`kinds=[23194]` subscription is lost forever.

### Fix
Store `pk:{pubkey}` as a JSON array, merging on write:
```rust
for pk in state.pubkeys {
    let key = format!("pk:{}", pk);
    let mut ids = self.read_pk_list(&key).await;
    if !ids.contains(&conn_id) {
        ids.push(conn_id);
    }
    entries.insert(key, serde_json::json!(ids));
}
```

Handle backward compatibility: old entries store a single number, new entries
store an array. `read_pk_list()` parses both formats.

### Lesson
- Any storage key that maps one-to-one must be reviewed for multi-connection
  scenarios. The `pk:` index was designed as "one wallet per pubkey" but
  the NWC protocol involves two connections (wallet + app) referencing the
  same pubkey with different subscription kinds.
- When designing storage schemas for Durable Objects, consider: can multiple
  connections legitimately reference this key?

---

## Bug 4: `global_latest` deduplication was too aggressive

**Date:** 2026-05-20, PR #13  
**Severity:** Critical (production — wallet blocked from receiving events)

### Symptom
Even after fixing Bug 3 (loading both connections' subscriptions), the
wallet still received no events. The `global_latest` check in `match_event`
prevented routing to any connection that wasn't the globally-latest
connection for a pubkey.

### Root Cause
`WalletIndex::match_event` required `global_latest == latest_conn`:

```rust
let global_latest = self.get_connection_id(&pk); // highest conn_id across ALL sub_keys
if global_latest == Some(*latest_conn) {           // must equal global latest
    sub_to_conns.insert(sub_key.0.clone(), vec![*latest_conn]);
}
```

`get_connection_id()` finds the max `conn_id` across **all** subscription
keys for a pubkey — even those with completely different filter patterns
that don't match the event.

For `pk=c231a264`:
- Wallet's sub_key: `(15:, [kinds=[23194], p=[c231a264]])` → latest_conn = 1779235076
- App's sub_key: `(sub:2, [kinds=[23195], authors=[c231a264]])` → latest_conn = 1779238076
- `global_latest` = max(1779235076, 1779238076) = 1779238076 (the app)

When routing a kind=23194 event:
- Wallet's sub_key matches (filter `kinds=[23194]` ✅)
- But `latest_conn` (1779235076) ≠ `global_latest` (1779238076) → **NOT ROUTED**

The `global_latest` check was designed to ensure "only the latest wallet
connection receives events" (NWC deduplication). But it compared across ALL
subscriptions, including unrelated ones — effectively allowing a later app
connection to **block** all event routing to the wallet.

### Fix
Deduplicate **only across matching subscriptions** per pubkey, not all
subscriptions:

```rust
let mut matching: Vec<(String, u32)> = Vec::new();
for sub_key in &entry.0 {
    if sub_key.1.iter().any(|f| f.matches(event)) {  // only matching subs
        if let Some(conns) = self.subscription_index.get(sub_key) {
            if let Some(&latest_conn) = conns.last() {
                matching.push((sub_key.0.clone(), latest_conn));
            }
        }
    }
}
// Only route to the latest connection among MATCHING subscriptions
let latest = matching.iter().map(|(_, id)| *id).max();
if let Some(latest_id) = latest {
    for (sub_id, id) in matching {
        if id == latest_id {
            sub_to_conns.insert(sub_id, vec![id]);
        }
    }
}
```

### Lesson
- When implementing deduplication, scope it correctly. "Only the latest
  connection should receive events" should mean "among connections with
  matching subscriptions," not "among all connections referencing this pubkey."
- Different NWC roles (wallet vs app) subscribe to the same pubkey for
  completely different purposes. Deduplication must consider whether
  subscriptions are semantically equivalent, not just whether they share a
  pubkey reference.

---

## Bug 5: `get_connection_id().is_none()` guard skipped `load_by_pubkey`

**Date:** 2026-05-20, PR #13  
**Severity:** Critical (prevented Bug 3 fix from working)

### Symptom
After fixing `pk:` to store arrays, events still showed `→ 0 subscribers`
when an in-memory connection already existed for the target pubkey.

### Root Cause
`WalletRegistry::match_event` had an optimization guard:

```rust
for pk in target_pks {
    if self.index.get_connection_id(&pk).is_none() {  // <-- GUARD
        if let Some(id) = self.load_by_pubkey(&pk).await {
            responses.push(RegistryResponse::WakeUp(id));
        }
    }
}
```

If ANY connection for the pubkey was already in the in-memory index (e.g.,
the app's connection, loaded by `wake_up_with_handler`), `load_by_pubkey`
was skipped entirely. The wallet's subscription (stored in `pk:{pubkey}`
as a second element in the array) was never loaded.

### Fix
Remove the guard — always call `load_by_pubkey` for each target pubkey.
`load_by_pubkey` itself handles already-loaded connections gracefully
(via `load()`'s early return on existing subscriptions).

### Lesson
- "Optimization" guards that skip loading based on partial knowledge are
  dangerous. If the in-memory index doesn't have ALL the data, the guard
  prevents completing the load.
- Always call `load_by_pubkey` — it's idempotent and fast enough.

---

## Bug 6: `load_by_pubkey()` early return on ANY in-memory connection

**Date:** 2026-05-20, PR #13  
**Severity:** Critical (prevented Bug 3 fix from working)

### Symptom
Even when `load_by_pubkey` was called, it returned immediately if ANY
connection for the pubkey was in memory, without checking storage for
additional connections.

### Root Cause
```rust
pub async fn load_by_pubkey(&mut self, pubkey: &str) -> Option<u32> {
    if let Some(id) = self.index.get_connection_id(pubkey) {
        return Some(id);  // <-- EARLY RETURN, only one conn_id
    }
    // ... never reaches storage if ANY conn is in memory
}
```

If the app's connection was in memory (loaded by the current message's
handler), this early return skipped storage lookup entirely, returning
only the app's ID — not the wallet's.

### Fix
Always read from storage. Only fall back to the in-memory index when
storage has no entry for this pubkey:

```rust
pub async fn load_by_pubkey(&mut self, pubkey: &str) -> Vec<u32> {
    let key = format!("pk:{}", pubkey);
    // Always read storage first to find ALL connections
    let storage_ids: Vec<u32> = if let Some(val) = self.storage.get(&key).await {
        // Parse old format (number) or new format (array)
        ...
    } else {
        // No storage entry, fall back to in-memory index
        if let Some(id) = self.index.get_connection_id(pubkey) {
            return vec![id];
        }
        return Vec::new();
    };
    // Load ALL listed connections
    for id in storage_ids {
        self.load(id).await;
    }
    ...
}
```

### Lesson
- Storage is the source of truth. The in-memory index is a cache.
  When discovering which connections exist for a pubkey, always consult
  storage first; the in-memory index may be incomplete after hibernation.

---

## Cross-Cutting Lessons

### 1. `wrangler dev` is not a Hibernation test
Local development uses miniflare/workerd which handles WebSocket events
differently from Cloudflare's production runtime. Critical differences:
- Attachment/accept ordering is not enforced locally
- Message routing works regardless of hibernation state
- **Any code change touching `accept_web_socket`, `serialize_attachment`,
  or WebSocket event handlers MUST be tested in production.**

### 2. Storage design for multi-connection scenarios
Durable Object storage keys that map one-to-one (e.g., `pk:{pubkey} → conn_id`)
break when multiple connections reference the same key. Always design for
one-to-many if there's any possibility of multiple connections.

### 3. In-memory index ≠ source of truth
After DO hibernation, the in-memory `WalletIndex` is empty. All subscription
data lives in DO storage. Code paths that consult only the in-memory index
(after checking if something is "already loaded") will miss data that hasn't
been restored yet — even if it exists in storage.

### 4. Order of operations in Cloudflare's Hibernation API
The sequence `serialize_attachment → accept_web_socket` is strictly required.
Calling in reverse silently corrupts state with no error or warning.

### 5. Deduplication scope
"Last-in-wins" deduplication must consider subscription semantics, not just
connection IDs. Two connections with the same pubkey but different subscription
kinds are NOT redundant — they serve different NWC roles (wallet receiving
requests vs app receiving responses).

---

## PR Timeline

| PR | Description | Status |
|----|-------------|--------|
| #9 | `accept_websocket_with_tags` approach | ❌ Broken — no `websocket_message` |
| #11 | `accept_web_socket` + `serialize_attachment` (wrong order) | ❌ Broken — no `websocket_message` |
| #12 | Swap order: `serialize_attachment` BEFORE `accept_web_socket` | ✅ `websocket_message` fires, but `→ 0 subscribers` |
| #13 | `pk:{pubkey}` arrays + dedup fix | ✅ Full routing restored |
