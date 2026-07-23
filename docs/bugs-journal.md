# Mekhala — Bug Journal

> **Note:** Mekhala has been ported from Rust/WASM to TypeScript. Entries below reference the original Rust codebase.

All bugs encountered during development, organized by domain. Each entry includes
symptom, root cause, fix, and lesson.

---

## Domain 1: DO Hibernation & Persistence

These bugs caused events to silently disappear or connections to become
unreachable after Cloudflare's Durable Object hibernation cycle.

---

### Hibernation Bug 1: `accept_websocket_with_tags` silently breaks message routing

**PR:** #9  
**Severity:** Critical (production-dead)

#### Symptom
- `websocket_message` handler never invoked in production
- Tags persisted correctly (`get_tags` confirmed)
- `wrangler dev` integration tests passed — messages routed fine locally
- Connections were accepted but received no events

#### Root Cause
Replaced custom `HibernationState` JS interop with `accept_websocket_with_tags(ws, tags)`.
While tags were stored, Cloudflare's production runtime did not route incoming
WebSocket messages to the `websocket_message` handler. The exact mechanism is
unclear (likely a runtime bug in tag-based routing at the time), but the result
was consistent: zero `websocket_message` invocations.

#### Fix
Use `accept_web_socket(ws)` + `serialize_attachment(&data)` instead.

---

### Hibernation Bug 2: `serialize_attachment` AFTER `accept_web_socket` — order corrupts routing

**PRs:** #11 (broken), #12 (fix)  
**Severity:** Critical (production-dead)

#### Symptom
- Connections registered: `✓ conn=N registered, attachment verified`
- `websocket_message` handler never invoked in production
- `wrangler dev` passed — messages routed fine locally
- Alby Go timed out with no `← RECV` logs

#### Root Cause
Calling `serialize_attachment(&id)` **after** `accept_web_socket(ws)`:

```rust
// BROKEN — PR #11:
state.accept_web_socket(ws);       // 1. Accept FIRST
ws.serialize_attachment(&id);       // 2. THEN set attachment

// WORKING — pre-PR-11:
server.serialize_attachment(&id)?;  // 1. Serialize FIRST
self.state.accept_web_socket(&server); // 2. THEN accept
```

In Cloudflare's production runtime, setting the attachment **after** acceptance
corrupts the WebSocket's Hibernation state. The attachment data IS stored, but
message routing silently breaks.

#### Why integration tests passed locally
`wrangler dev` (miniflare/workerd) does not exercise the Hibernation API the
same way as production. Locally, WebSocket events are routed through an
internal event loop regardless of accept/attachment order. Only production
enforces the full Hibernation API contract.

#### Lesson
`serialize_attachment` MUST be called before `accept_web_socket`. Always.
No exceptions. `wrangler dev` cannot catch Hibernation API ordering bugs.

---

### Hibernation Bug 3: `pk:{pubkey}` storage overwrite — only one connection per pubkey

**PRs:** #13 (fix), #21 (regression in PR #19)  
**Severity:** Critical (production — events not routed after hibernation)

#### Symptom
- `websocket_message` fired (Bug 2 fixed)
- `event kind=23194 → 0 subscribers` despite wallet having an active subscription
- Wallet (conn=5076) subscribed `kinds=[23194] #p=c231a264`
- App (conn=8076) subscribed `kinds=[23195] authors=c231a264`
- After hibernation, only app's subscription restored
- Wallet's subscription undiscoverable → events silently dropped

#### Root Cause (original — PR #13)
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
(the app). The wallet's subscription is lost forever.

#### Fix (PR #13)
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

`read_pk_list()` parses both old (single number) and new (array) formats.

#### Regression (PR #19 → PR #21)
PR #19 removed `sync()` entirely from `subscribe()`/`unsubscribe()` as a
performance optimization, deferring persistence to `on_terminate()`. But
`on_terminate()` only wrote `conn:{id}` — it **never wrote `pk:{pubkey}`**.
The in-memory `pk_cache` worked during the DO's lifetime, but after
hibernation the `pk:` storage entries were stale or missing entirely.

PR #21 restored `sync()` with proper `pk:{pubkey}` array writes on every
`subscribe()`/`unsubscribe()` call, fixing the regression.

#### Lesson
Any storage key that maps one-to-one must be reviewed for multi-connection
scenarios. The NWC protocol involves two connections (wallet + app)
referencing the same pubkey with different subscription kinds. Storage is the
source of truth — in-memory caches that skip storage writes will break on
hibernation.

---

### Hibernation Bug 4: `global_latest` deduplication was too aggressive

**PR:** #13  
**Severity:** Critical (production — wallet blocked from receiving events)

#### Symptom
Even after fixing Bug 3 (loading both connections' subscriptions), the wallet
still received no events. The `global_latest` check in `match_event` prevented
routing to any connection that wasn't the globally-latest connection for a pubkey.

#### Root Cause
`WalletIndex::match_event` required `global_latest == latest_conn`:

```rust
let global_latest = self.get_connection_id(&pk); // highest conn_id across ALL sub_keys
if global_latest == Some(*latest_conn) {
    sub_to_conns.insert(sub_key.0.clone(), vec![*latest_conn]);
}
```

`get_connection_id()` finds the max `conn_id` across **all** subscription keys
for a pubkey — even those with completely different filter patterns that don't
match the event.

For `pk=c231a264`:
- Wallet's sub_key: `(15:, [kinds=[23194], p=[c231a264]])` → latest_conn = 1779235076
- App's sub_key: `(sub:2, [kinds=[23195], authors=[c231a264]])` → latest_conn = 1779238076
- `global_latest` = max(1779235076, 1779238076) = 1779238076 (the app)

When routing a kind=23194 event:
- Wallet's sub_key matches (filter `kinds=[23194]` ✅)
- But `latest_conn` (1779235076) ≠ `global_latest` (1779238076) → **NOT ROUTED**

#### Fix
Deduplicate only across matching subscriptions per pubkey:

```rust
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
```

#### Lesson
Deduplication scope matters. "Only the latest connection receives events"
should mean "among connections with matching subscriptions," not "among all
connections referencing this pubkey." Different NWC roles (wallet vs app)
subscribe to the same pubkey for completely different purposes.

---

### Hibernation Bug 5: `get_connection_id().is_none()` guard skipped `load_by_pubkey`

**PR:** #13  
**Severity:** Critical (prevented Bug 3 fix from working)

#### Symptom
After fixing `pk:` to store arrays, events still showed `→ 0 subscribers`
when an in-memory connection already existed for the target pubkey.

#### Root Cause
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
was skipped entirely. The wallet's subscription was never loaded.

#### Fix
Remove the guard — always call `load_by_pubkey` for each target pubkey.

---

### Hibernation Bug 6: `load_by_pubkey()` early return on ANY in-memory connection

**PR:** #13  
**Severity:** Critical (prevented Bug 3 fix from working)

#### Root Cause
```rust
pub async fn load_by_pubkey(&mut self, pubkey: &str) -> Option<u32> {
    if let Some(id) = self.index.get_connection_id(pubkey) {
        return Some(id);  // early return, only one conn_id
    }
    // never reaches storage if ANY conn is in memory
}
```

#### Fix
Always read from storage first. Only fall back to the in-memory index when
storage has no entry for this pubkey.

---

### Hibernation Bug 7: `active_wallets` state lost on DO hibernation wake

**Commit:** `0b1c5d2`  
**Severity:** High

#### Symptom
After DO hibernation, the `active_wallets` tracking set was empty, causing
the relay to lose knowledge of which wallet pubkeys were active. This
affected info event lifecycle management.

#### Root Cause
The `active_wallets` HashSet was stored only in memory on the `NwcRelay`
struct in `lib.rs`. When the DO hibernated, this in-memory state was lost.
On wake, the set was empty, so `is_wallet_active()` checks returned false.

#### Fix
On DO wake (`load_pks_from_storage`), reconstruct `active_wallets` by
iterating `state.storage().list()` to find all `conn:{id}` entries, load
each connection's subscription state, and re-seed `active_wallets` from
the info events found during restoration.

Also expanded tracking to `authors` — when a connection subscribes with an
`authors` filter matching a known wallet pubkey, that pubkey is also tracked.

---

## Domain 2: Client & Protocol Compatibility

Bugs that caused client timeouts, silent failures, or protocol mismatches.

---

### Bug 8: No OK message for kind 13194 — client timeout

**PR:** #3  
**Severity:** High (client timeout)

#### Symptom
Clients (e.g., Alby Hub) publishing a NIP-47 info event (kind 13194) would
time out waiting for the OK response. The event was stored and processed,
but no confirmation was sent back to the client.

#### Root Cause
The EVENT handler for kind 13194 performed async storage operations before
sending the OK response. The client's timeout expired during the async I/O.

#### Fix
Send the OK response for kind 13194 **immediately** (before any async
storage), similar to how other EVENT messages are handled. The client
receives confirmation and can proceed, while the relay finishes persistence
in the background.

---

### Bug 9: Alby Hub timeouts and Info event lifecycle

**PR:** #4  
**Severity:** High (Alby Hub compatibility)

#### Symptom
- Alby Hub connections frequently timed out
- Info events (kind 13194) were not reliably delivered
- Limits on content length and event tags were too restrictive for real wallets

#### Root Cause
Multiple issues combined:
1. Ok response timing was still too slow for Alby Hub's expectations
2. Info event lifecycle was fragile — info events weren't re-published on
   subscription or reliably cached for discovery
3. Limits (`MAX_CONTENT_LENGTH`, `MAX_EVENT_TAGS`) were set too low for
   production wallet configurations

#### Fix
- Increase `MAX_EVENT_TAGS` and `MAX_CONTENT_LENGTH` to NIP-47-aligned values
- Improve info event caching and delivery lifecycle
- Streamline the EVENT response path to minimize latency

---

### Bug 10: NWC timeout, tag fidelity loss, filter validation gaps

**PR:** #5  
**Severity:** High (NWC protocol correctness)

#### Symptom
Multiple NWC interop issues:
1. Alby Hub timed out waiting for EVENT OK responses
2. Tags were silently corrupted — JSON string values were converted to
   strings even when they should be arrays or numbers
3. NIP-47 limits were not properly enforced
4. NWC requests with valid `p`/`e` tag narrowing were rejected by
   `Filter::is_valid()`

#### Root Causes & Fixes

**Fix 1: Send OK immediately**
Before async storage, the EVENT handler would wait for I/O. Fix: send OK
before any `.await` call, matching NIP-47 expectations.

**Fix 2: Tag fidelity**
Tags used `Vec<String>`, but NIP-47 tags often contain non-string values
(e.g., `["p", "<hex>", "<relay>"]` where the relay is optional). Some
JSON parsers output `Value::Null` for missing fields. Changed to
`Vec<serde_json::Value>` for faithful JSON round-trip, preserving the
original tag structure exactly as received.

**Fix 3: NIP-47 aligned limits**
`MAX_EVENT_TAGS` raised from 10 to 100. `MAX_CONTENT_LENGTH` raised from
16384 to 65536 (64 KB). These match NIP-47 spec.

**Fix 4: NWC kind requirement bypass**
`Filter::is_valid()` required all filters to have `kinds` specified. But
NWC subscriptions often narrow by `#p`, `#e`, or `ids` without explicitly
listing kinds. Fix: bypass the `kinds` requirement when other narrowing
tags are present. Added `handle_req_internal()` for internal RPC calls
that bypass this validation entirely.

---

### Bug 11: `#p` filter doesn't match info events (kind 13194)

**PR:** #17  
**Severity:** High (NIP-47 client discovery broken)

#### Symptom
When a client subscribes with `{kinds: [13194], "#p": [walletPk]}`,
`filter.matches()` checks `event.tagged_pubkeys()`. But NIP-47 info
events (kind 13194) carry no `p`-tags — the event's pubkey **is** the
wallet pubkey. The match returned false, so the info event was never
delivered.

Clients following NIP-47 discovery (`#p` filter on info events) could not
find wallets.

#### Root Cause
`Filter::matches()` checked tag-based `p` values only. It had no awareness
that for kind 13194, the event's own `pubkey` field serves the same role
as a `p`-tag.

#### Fix
In `Filter::matches()`, if the filter includes `#p` constraints and the
event kind is 13194 (or kinds are unspecified, allowing all), check if
the event's own pubkey matches the `#p` value:

```rust
// In filter.matches():
if let Some(p_tags) = &self.p_tags {
    let matches_tag = event.tagged_pubkeys().iter().any(|pk| p_tags.contains(pk));
    let is_info_event = event.kind == 13194;
    let matches_own_pk = is_info_event && p_tags.contains(&event.pubkey);
    if !matches_tag && !matches_own_pk {
        return false;
    }
}
```

**Important safeguard:** Only apply the `#p` → own-pubkey fallback when
the filter's kinds (if present) include 13194. Otherwise, a wallet
subscription with `{kinds: [23194], #p: [walletPk]}` would also match
info events, causing incorrect routing.

---

## Domain 3: Concurrency & Race Conditions

Bugs where concurrent handler invocations caused panics or dropped events.

---

### Bug 12: Connection-not-found race condition

**PR:** #8  
**Severity:** Critical (connection state corruption)

#### Symptom
Intermittent `"Connection not found"` errors in production. A connection
would be accepted but then immediately unreachable when a message arrived.

#### Root Cause
The WebSocket accept sequence was split across async awaits:

```rust
let id = self.next_id();  // allocate ID
do_some_async_thing().await;  // <-- other handler could accept a WS here
self.accept_web_socket(ws);   // too late — ID belongs to wrong connection
```

Between ID allocation and `accept_web_socket`, another concurrent DO
handler could accept a different WebSocket, consuming the same slot or
creating a race on the connection registry.

#### Fix
Make WebSocket acceptance atomic with ID allocation — all before any
`.await`:

```rust
fn accept_and_register(&self, conn: WebSocket) -> u32 {
    let id = self.allocate_id();           // 1. Allocate ID first
    self.state.accept_web_socket(conn);    // 2. Accept immediately
    self.add_active(id);                   // 3. Register in-memory
    ws.serialize_attachment(&id)?;         // 4. Serialize for hibernation
    id
}
```

The entire sequence runs synchronously before any `.await`, eliminating
the race window.

---

### Bug 13: `RefCell` borrow across `.await` — `BorrowMutError` panic

**PR:** #8  
**Severity:** Critical (process crash — `panic = "abort"`)

#### Symptom
Intermittent panics with `BorrowMutError` — "already borrowed:
BorrowMutError". Because `panic = "abort"` is configured, this crashes the
entire DO isolate.

#### Root Cause
The connection ID counter and WebSocket registry lived inside a
`RefCell<WebSocketRegistry>` on `CloudflareTransport`. The typical
pattern was:

```rust
let id = self.registry.borrow_mut().next_id();
some_async_storage_op().await;      // RefCell borrow still active!
self.registry.borrow_mut().accept(ws);  // BOOM: already borrowed mut
```

`RefCell` borrows are not `Send`, but in a single-threaded WASM context
the issue is different: the `borrow_mut()` guard is dropped at the end of
the scope, but if another DO handler runs during the `.await` (via
interleaving), it also tries to `borrow_mut()` — which panics because
the first borrow is still active across the await point.

#### Fix
Move the ID counter out of `RefCell` into a dedicated `Mutex<IdCounterState>`
on `CloudflareTransport`. The `allocate_id()` method locks the mutex,
allocates, and drops the guard — all synchronously, before any `.await`:

```rust
struct IdCounterState {
    current_id: u32,
    max_connections: u32,
}

impl CloudflareTransport {
    fn allocate_id(&self) -> Result<u32, RelayError> {
        let mut state = self.id_counter.lock().unwrap();
        if state.current_id >= state.max_connections {
            return Err(RelayError::Generic("max connections reached".into()));
        }
        let id = state.current_id;
        state.current_id += 1;
        Ok(id)
    }
}
```

The `WebSocketRegistry` (still in `RefCell`) is only accessed in short,
synchronous bursts that don't cross `.await` boundaries.

#### Lesson
In a single-threaded WASM runtime with `panic = "abort"`, any `RefCell`
borrow held across an `.await` point can cause a fatal panic if another
handler interleaves. Use `Mutex` for state that must be accessed across
await boundaries, or ensure borrows are scoped to drop before `.await`.

---

## Domain 4: Build & Infrastructure

Bugs that broke the build pipeline, deployment, or local development setup.

---

### Bug 14: `worker-build` not in PATH inside `wrangler` build context

**Commits:** `a7ca581`, `ddc67ad`  
**Severity:** High (build broken for fresh setups)

#### Symptom
`wrangler build` failed with `worker-build: command not found` even though
`worker-build` was installed globally. The `wrangler.toml` `[build]` section
called `./scripts/build.sh` but the PATH inside wrangler's execution context
did not include the directory where `worker-build` was installed.

#### Root Cause
`wrangler` sanitizes the PATH when executing build commands, stripping
non-standard directories. Rust/cargo-installed binaries like `worker-build`
live in `~/.cargo/bin/` which was removed from PATH inside wrangler's
subprocess.

#### Fix
Two-step fix:
1. `ddc67ad`: Create `build.sh` wrapper that explicitly extends PATH:
   ```bash
   # build.sh
   export PATH="$HOME/.cargo/bin:$PATH"
   worker-build
   ```
2. `a7ca581`: Change `wrangler.toml` to reference `build.sh` via worker-build
   command directly to ensure PATH is set before worker-build runs.

#### Lesson
Cloudflare Workers build context has a restricted PATH. Any tool installed
via `cargo install` (or similar non-system paths) must be made available
explicitly. Use a build script wrapper rather than relying on PATH
inheritance.

---

### Bug 15: `new_sqlite_classes` migration for Cloudflare Free Tier DO support

**Commit:** `6418582`  
**Severity:** High (deployment failed on Free Tier)

#### Symptom
Deploying to Cloudflare Free Tier (non-paid Workers plan) failed with an
error about SQLite Durable Object migration format. The DO was unable to
initialize its storage backend.

#### Root Cause
The `wrangler.toml` `[[migrations]]` section used the default DO migration
type, which requires the Workers Paid plan. Cloudflare Free Tier uses a
different SQLite-based DO storage backend that requires the
`new_sqlite_classes` migration type.

#### Fix
Change the migration type in `wrangler.toml`:
```toml
[[migrations]]
tag = "v1"
new_sqlite_classes = ["NwcRelay"]
```

#### Lesson
Cloudflare Free Tier Durable Objects use a SQLite storage backend with a
different migration format. Always check which DO migration type matches
the target deployment plan (`new_sqlite_classes` for Free Tier /
`new_classes` for Paid).

---

### Bug 16 (Diagnostic): Info event delivery failure logging

**PR:** #16  
**Nature:** Not a fix — diagnostic scaffolding that enabled Bug 11 fix

#### Context
Between PR #13 (hibernation routing fixed) and PR #17 (#p filter fix), info
events (kind 13194) were not being delivered to clients after hibernation.
The exact failure point was unclear from production logs.

#### What was added
- Reorder `accept_new_connection` to ensure the 'accepted' log line fires
  before WebSocket response evaluation
- Add send success/failure logging to `WebSocketRegistry::send`
- Add hibernation recovery logging to `find_by_id`: scanned count, recovery
  success, NOT FOUND warnings
- Add dispatch fallthrough warning when both `send_to_peer` and internal
  channel fail

#### Outcome
These logs revealed that connections were waking from hibernation but info
events were not being matched — the actual bug was the `#p` filter mismatch
(fixed in PR #17, Bug 11).

#### Lesson
When production behavior differs from local tests, add **targeted
diagnostic logging** at each decision point in the suspected code path.
Ship the logging, observe production, identify the exact failing check,
then fix and remove (or reduce) the logs.

---

## Cross-Cutting Lessons

### 1. `wrangler dev` is not a Hibernation test
Local development uses miniflare/workerd which handles WebSocket events
differently from Cloudflare's production runtime. Critical differences:
- Attachment/accept ordering is not enforced locally
- Message routing works regardless of hibernation state
- Any code change touching `accept_web_socket`, `serialize_attachment`,
  or WebSocket event handlers MUST be tested in production.

### 2. Storage design for multi-connection scenarios
Durable Object storage keys that map one-to-one (e.g., `pk:{pubkey} → conn_id`)
break when multiple connections reference the same key. Always design for
one-to-many if there's any possibility of multiple connections.

The NWC protocol inherently involves two connections (wallet + app) per pubkey.

### 3. In-memory index ≠ source of truth
After DO hibernation, the in-memory `WalletIndex` is empty. All subscription
data lives in DO storage. Code paths that consult only the in-memory index
(after checking if something is "already loaded") will miss data that hasn't
been restored yet — even if it exists in storage. Always consult storage first.

### 4. Order of operations in Cloudflare's Hibernation API
The sequence `serialize_attachment → accept_web_socket` is strictly required.
Calling in reverse silently corrupts state with no error or warning.
`wrangler dev` will not catch this.

### 5. Deduplication scope
"Last-in-wins" deduplication must consider subscription semantics, not just
connection IDs. Two connections with the same pubkey but different subscription
kinds are NOT redundant — they serve different NWC roles (wallet receiving
requests vs app receiving responses).

### 6. In-memory caches must not skip storage writes
PR #19's `pk_cache` optimization deferred `pk:{pubkey}` storage writes
entirely. This worked during the DO's lifetime but caused complete routing
failure after hibernation. Storage is the source of truth; in-memory caches
are supplementary, not a replacement.

### 7. `RefCell` across `.await` is unsafe with interleaving
In a single-threaded WASM runtime, `RefCell` borrows held across `.await`
can cause `BorrowMutError` panics (→ process abort). Use `Mutex` for
cross-await state, or scope borrows to drop before `.await`.

### 8. Tag fidelity requires `Value`, not `String`
JSON tags in NIP-47 may contain non-string values (`null`, numbers) or
optional fields. Using `Vec<String>` silently corrupts these. Always use
`Vec<serde_json::Value>` for faithful round-trip.

### 9. Send OK before async
NIP-47 clients expect immediate OK responses for EVENT messages. Any
async I/O (storage, network) before sending OK will cause client timeouts.
Pattern: send OK → `.await` for storage.

### 10. `#p` filter for info events must check `event.pubkey`
Kind 13194 info events carry no `p`-tags — the event's own pubkey IS the
wallet pubkey. The `#p` filter match must special-case kind 13194 to check
`event.pubkey` against the filter's `#p` values. But constrain this: only
when kinds explicitly include 13194, to avoid misrouting NWC requests.

---

## PR Timeline

| PR / Commit | Description | Outcome |
|-------------|-------------|---------|
| `a7ca581` + `ddc67ad` | `worker-build` not in PATH | ✅ Build wrapper fix |
| `6418582` | `new_sqlite_classes` for Free Tier DO | ✅ Deploy on Free Tier |
| #3 | Return OK for kind 13194 | ✅ Client timeout fixed |
| #4 | Alby Hub limits + Info lifecycle | ✅ Alby Hub compatible |
| #5 | NWC timeout, tag fidelity, filter validation | ✅ NWC protocol correct |
| #8 | Race-free WS accept + RefCell→Mutex | ✅ No BorrowMutError, no races |
| #9 | `accept_websocket_with_tags` | ❌ No `websocket_message` |
| #11 | `accept_web_socket` + serialize (wrong order) | ❌ No `websocket_message` |
| #12 | serialize BEFORE accept | ✅ `websocket_message` fires |
| #13 | pk: arrays + dedup fix | ✅ Full routing after hibernation |
| #16 | Diagnostic logging for info delivery | 🔍 Found Bug 11 root cause |
| #17 | `#p` filter for info events | ✅ NIP-47 discovery works |
| #19 | pk_cache / VK cache optimizations | ❌ Broke hibernation (reverted) |
| #21 | Restore pk: writes in sync() | ✅ Hibernation routing restored |
| `0b1c5d2` | `active_wallets` lost on hibernation wake | ✅ State restored on wake |

---

---

### Structured Clone Bug: `Tag` class loses shape through DO storage

**PR:** `fix/tag-serialization-structured-clone`  
**Severity:** Critical (Alby Go "no info event" timeout)

#### Symptom
- Alby Go SDK times out with "no info event (kind 13194) returned from relay"
- Hub **does** publish kind 13194 to all configured relays (confirmed via code trace)
- `cacheInfo()` stores the event, `getInfo()` retrieves it, but JSON wire format has `"tags":[{"raw":["encryption","nip04"]}]` instead of `"tags":[["encryption","nip04"]]`
- SDK's `parseEventJSON` calls `Tag.fromJSON(t)` on each tag, but `t` is `{raw:[...]}` not `["encryption",...]`; `t[0]` is `undefined`, `encryptionScheme()` returns `null`, `onevent` callback never fires → 10s timeout

#### Root Cause
The `Tag` class stores its array data in a private `raw` property:

```ts
export class Tag {
  private constructor(private raw: JsonValue[]) {}

  toJSON(): JsonValue[] {
    return this.raw;  // used by JSON.stringify, NOT by structured clone
  }
}
```

`DurableObjectStorage.putBatch()` uses the **structured clone** algorithm, which does **not** call `toJSON()`. Instead, it clones own properties. A `Tag` instance becomes `{raw: ["encryption","nip04"]}` in storage — a plain object, not an array.

On retrieval via `storage.get()`, the cloned object retains its `{raw: [...]}` shape. When `WalletRegistry.getInfo()` returned this as `Event`, `serializeEvent()` → `JSON.stringify` produced malformed JSON. The SDK parses the event's tags expecting arrays (`Tag.fromJSON` operates on `arr[0]`), finds objects, and cannot extract the encryption scheme.

#### Why existing tests passed
`MockStorage` uses a `Map` internally, which preserves object references. The `simulateHibernation` helper copies data via `new Map()` — no structured clone occurs. The bug only manifests in the real `DurableObjectStorage` environment.

#### Fix
Convert `Tag[]` to `JsonValue[][]` (plain arrays) at every persistence boundary:

1. **`WalletRegistry.cacheInfo()`** — map `t.getRaw()` before `putBatch`
2. **`WalletRegistry.getInfo()`** — reconstruct `Tag` instances from stored data (handles both plain-array and legacy `{raw}` formats)
3. **`WalletIndex.save()`** — convert tags before embedding in state JSON
4. **`WalletIndex.restore()`** — reconstruct `Tag` instances from stored state

Unit tests (`wallet-registry.test.ts`) simulate the `{raw}` format by directly corrupting stored tags and verify:
- `encryptionScheme()` returns the correct value after round-trip
- `JSON.stringify(serializeEvent(...))` produces `["encryption","nip04"]` not `{"raw":["encryption","nip04"]}`

Integration tests (`info-event.test.js`) verify the full wire format: tags are plain arrays in the WebSocket JSON message.

#### Lesson
**Always convert class instances to plain JSON-compatible structures before storing in DO storage.** Structured clone does not preserve class shape, prototype, or `toJSON()` behavior. This applies to `DurableObjectStorage.putBatch()`, `ctx.storage.put()`, and any WebSocket attachment serialization.

Test mocks that use in-memory maps (`Map`, `Record`) will **not** catch structured clone bugs. To validate, either:
1. Use `JSON.parse(JSON.stringify(data))` to simulate structured clone (but only if `toJSON()` is not the fix)
2. Or directly corrupt stored data to the expected deserialized format
3. Or run integration tests against a real DO environment (wrangler dev)

## PR Timeline Update

| PR / Commit | Description | Outcome |
|-------------|-------------|---------|
| `fix/tag-serialization-structured-clone` | Convert `Tag[]` to plain arrays before DO storage | ✅ Tags survive structured clone round-trip |

## Updated Heuristics

12. **Are class instances being stored in DO storage?** → Convert to plain arrays/objects first. `toJSON()` is irrelevant — structured clone doesn't call it. Test with `JSON.parse(JSON.stringify(data))` or by corrupting mock storage data directly.
