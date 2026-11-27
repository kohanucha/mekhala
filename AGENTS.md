# Mekhala Agent Context

## Overview
Cloudflare Worker (Rust/WASM) implementing Nostr Wallet Connect (NIP-47) relay with WebSocket Hibernation.

The relay only validates and routes events. It never decrypts NIP-47 content
and holds no wallet credentials. Lightning Address lives in a separate
self-hosted service (manee) that connects to this relay as an ordinary NWC
client. **Do not add Lightning Address, LNURL, wallet-session or NWC-client
code here**: anything that needs a wallet's secret belongs on the client side.

This file is the only agent instruction file in the repository.

## Key Commands
| Action | Command | Notes |
|--------|---------|-------|
| Build WASM | `./scripts/build.sh` | Requires Rust + wasm32 target + worker-build |
| Dev server | `npx wrangler dev` | Local on port 8787 |
| Unit tests | `cargo test` | Rust-only tests |
| Format | `cargo fmt --all -- --check` | rustfmt defaults, no `rustfmt.toml`; CI fails on drift |
| Integration tests | `./scripts/test.sh` | Full pipeline: test → build → wrangler → node |

## Build Requirements
- Toolchain is pinned in `rust-toolchain.toml` (1.100.0, includes the
  `wasm32-unknown-unknown` target); `rustup` installs it on first use. CI uses
  the same version, so local clippy matches the gate.
- `cargo install worker-build`
- Node.js + npm

## Architecture
- **Durable Object**: `CloudflareTransport` (`src/cloudflare/transport.rs`; wrangler `class_name = "CloudflareTransport"`)
- **Modules**: auth.rs, lib.rs, cloudflare/, nostr/, util/ (`common/` is test fixtures only)

## Critical Dependencies
- `worker` 0.8.x runtime
- `k256` for Schnorr signatures

## Required Env Vars
- `AUTH_RELAY_SECRET` - password (set via Cloudflare dashboard). Without it the
  relay refuses every request with 404 and logs why: a missing secret closes
  the relay, it never opens it
- `AUTH_PUBLIC` - optional: exactly `true` runs a public relay when no secret is
  set; any other value keeps it closed. A secret, when set, wins
- `DO_REGION` - optional: apac/weur/wnam (default: apac)
- `CONN_MAX_TOTAL` - optional, default: 100
- `CONN_MAX_PER_IP` - optional, default: 5, `0` disables (see
  "Connection admission" below)
- `CONN_MAX_PER_ASN` - optional, default: 20, `0` disables
- `CONN_MAX_MESSAGES_PER_MINUTE` - optional, default: 120, `0` disables (see
  "Inbound message budget" below)
- `CONN_IDLE_TIMEOUT_SECONDS` - optional, default: 120, `0` disables the sweep
  (connections idle longer than this *and* holding no subscription are closed)
- `CONN_SWEEP_SECONDS` - optional, default: 60, `0` disables the sweep
- `CONN_EVICT_MIN_IDLE_SECONDS` - optional, default: 10: minimum idle time before a
  subscription-less connection may be closed to free a slot
- `CONN_ORPHAN_SWEEP_SECONDS` - optional, default: 3600, `0` disables: how often the
  alarm removes the stored state of connections whose socket is gone (see
  "Connections lost without a close" below)
- `SUB_MAX_PER_CONN` - optional, default: 100
- `SUB_MAX_FILTER_VALUES` - optional, default: 10 (max values per filter field)
- `SUB_MAX_ROUTING_KEYS` - optional, default: 1000
- `EVENT_MAX_CONTENT_BYTES` - default: 98304 (96 KB, covers the 87,472-character NIP-44 ceiling)
- `EVENT_MAX_TAGS` - optional, default: 10
- `INFO_CACHE_MAX_EVENTS` - optional, default: 1000, `0` disables the cap (see
  "Info cache" below)
- `INFO_CACHE_TTL_SECONDS` - optional, default: 2592000 (30 days), `0` disables
  expiry

## Wrangler Config
- Output: `build/worker/shim.mjs`
- Durable Object binding: `MEKHALA_NWC_DO` → class `CloudflareTransport`
- Compatibility date: 2026-04-30
- `workers_dev = false` and `preview_urls = false`: the Worker is public only
  through the custom domain bound in the dashboard. Do not flip these on to
  "test" a deploy — preview URLs would expose unreleased code publicly.

## Connection admission

Every WebSocket upgrade passes three gates in
`CloudflareTransport::accept_new_connection`, in this order:

1. `CONN_MAX_PER_IP` — connections sharing one address tag.
2. `CONN_MAX_PER_ASN` — connections sharing one autonomous system.
3. `CONN_MAX_TOTAL` — the relay total. On reaching it, the longest-idle
   connection **that holds no subscription** is closed to free a slot (only
   when it has been idle at least `CONN_EVICT_MIN_IDLE_SECONDS`); with no such
   connection the upgrade gets 429.

The identity is decided in the Worker, not in the Durable Object: `connect()`
reads `cf.asn()` and `CF-Connecting-IP`, hashes the address into a fixed-length
tag, then sets `x-mekhala-client-ip-tag` / `x-mekhala-client-asn` on the
forwarded request after deleting any client-supplied value under those names
(`src/cloudflare/identity.rs`). Per-address counting is therefore only as
trustworthy as `CF-Connecting-IP`; `asn` is edge-derived and is the
rotation-resistant backstop.

Identity and a last-seen timestamp ride on the WebSocket attachment
(`serialize_attachment`), so they survive hibernation at no storage cost; the
timestamp is refreshed at most every 30 s. `CONN_IDLE_TIMEOUT_SECONDS` closes
connections that hold no subscription and have been silent that long, driven
by the Durable Object alarm (`alarm()`), which re-arms itself every
`CONN_SWEEP_SECONDS` while sockets exist.

### Connections lost without a close

A deploy, an eviction or a crash drops every socket of the old instance, and
the new instance never gets a close event for them. Their `conn:` state and
`pk:`/`eid:`/`etag:` memberships would otherwise stay forever: `load_indexed`
only prunes ids whose `conn:` is missing, and here it is present. Two paths
remove them:

- **On the next event** (`process_responses`): a `WakeUp` whose socket is not
  in `get_websockets()` calls `on_terminate` and skips any send to that id.
  Hibernated sockets stay listed, so a missing one is really gone.
- **On the alarm** (`WalletRegistry::prune_orphaned_connections`): compares the
  `conn:` keys with the live socket ids and removes the rest, for subscribers
  no event ever reaches. A full pass runs at most once per
  `CONN_ORPHAN_SWEEP_SECONDS` (one `get` of `orphan_sweep_at` otherwise) and removes
  at most 128; a pass cut short by that limit is not recorded, so the next
  alarm continues. The sweep holds the engine lock, so a new connection cannot
  save state between reading the sockets and comparing them with storage.

Trade-offs: per-address caps put CGNAT and corporate NAT users in one bucket,
so keep them generous (or disable them) where that matters; a wallet that is
merely idle keeps its subscription and is never swept.

## Info cache

`kind` 13194 events are cached so a hibernated or freshly connected client can
still answer a wallet-discovery `REQ`. Each cached event occupies exactly two
keys, `info:<author>` (the event) and `infoid:<event id>` (reverse mapping).

Each event is limited to `MAX_INFO_EVENT_BYTES` (4 KiB of stored JSON,
`src/nostr/limits.rs`); a larger one gets `OK false` before its signature is
checked. A real info event is a method list and two short tags, well under
1 KiB, while the general content limit is 96 KiB for NIP-44 requests. Without
this cap, 1,000 cached events could hold ~128 MB, and the enumeration below
would exceed the Durable Object's memory on every alarm.

The number of events is bounded by two independent settings:

- `INFO_CACHE_MAX_EVENTS` caps how many events are kept. Enforced on publish
  (`cache_info`) because the publisher decides how many keys exist; one publish
  over the cap trims back to 90% of it so a burst pays for one enumeration
  rather than one per publish. Eviction drops the oldest `created_at` first,
  and takes the `infoid:` mapping with it.

  Because the publisher also controls how often that check runs, it may not
  cost a listing per publish: `WalletRegistry` keeps an in-memory `info_count`
  hint, and only an unknown hint (first publish after a wake) or a hint at the
  cap triggers the enumeration. The hint is reseeded by every enumeration and
  moved by every write and delete. A hint that drifts high only forces a
  redundant listing; one that drifts low would weaken the cap — which is why
  the alarm reseeds it instead of trusting it forever.
- `INFO_CACHE_TTL_SECONDS` expires events, measured from the event's own
  `created_at`. Enforced on read (`get_info` treats an expired event as absent
  and deletes it) and on the Durable Object alarm, so a cache nobody reads
  still shrinks.

Both bounds run through `WalletRegistry::trim_info_cache`, which needs an
enumeration — `Storage::list_page("info:", ...)` — because the number of cached
wallets is not tracked anywhere that survives hibernation. It reads 128 entries
per page and keeps only each event's author, id and `created_at`, so memory is
one page however large the cache is. The same pass deletes events over the
size cap that were stored before it existed. `0` disables either bound;
setting both to `0` removes the enumeration entirely.

Kind 13194 is replaceable (NIP-01), so `cache_info` only replaces the cached
event with a newer one: a later `created_at` wins, and on a tie the lower id.
Anyone who saw an older info event can send it again with its valid
signature; without this rule it would roll the cache back to capabilities or
an encryption the wallet no longer uses. An event that does not replace the
cached one is still answered `OK true` and routed to live subscribers, but
not stored; sending the cached event again costs no write.

Known limitation: a deletion (kind `5`) leaves nothing behind, so the deleted
info event can be cached again by sending it once more, until it expires by
`INFO_CACHE_TTL_SECONDS`. A tombstone in `info:<author>` would close that, at
the cost of a second value shape in every reader of the family and a rule to
expire tombstones when both bounds are `0`; it was left out on purpose. A
wallet's own new info event is unaffected, since it is signed with a later
`created_at`.

Trade-off: TTL is measured against `created_at`, which the publisher controls.
A wallet that republishes keeps its entry; one that never republishes
disappears after 30 days and its discovery `REQ` simply returns nothing, which
is the same answer the relay gave before the event was ever published.

### Scope: author-only discovery

The cache is keyed by author, so a `REQ` can only replay an event whose author is
one of the filter's values. `{kinds:[13194], "#p":[<app pubkey>]}` replays
nothing and just EOSEs, and `Filter::matches` still requires a real `p` tag, so
even `#p:[<wallet pubkey>]` matches only an info event that carries that tag.
That is deliberate: the wallet-auth / one-click Nostr flow, where a wallet
introduces itself to an app it has never met, is out of scope. Adding it back
means indexing `p` tags again, which the two-key layout above avoids (a single
alias key per app would let any publisher hide another wallet, and replaying
every announcement for an app multiplies the bytes one `REQ` can pull).

A live publish is unaffected: it is routed like any other NIP-01 event, so an
app already subscribed when the wallet publishes still receives it. Only
replay — the case where the app subscribes after the fact — has no path.

Keys written by the version that did index `p` tags (`info-p:`) are deleted by
`prune_retired_keys()`, which the alarm calls. The same sweep removes the
`rate:` counters of the Lightning Address callback limiter that used to live in
this relay. Nothing writes either family any more, so the sweep is a one-way
migration: at most 128 keys per pass across both families, and its "already
empty" case is one `list_page` per family. To retire another key family,
add its prefix to `RETIRED_KEY_PREFIXES` in `src/nostr/wallet_registry.rs`.

## Inbound message budget

Every WebSocket frame a client sends is charged against a per-connection budget
(`CONN_MAX_MESSAGES_PER_MINUTE`, default 120) before it is parsed or allowed to
touch storage. Past the budget the relay sends one `NOTICE` ("rate limited,
retry in Ns") and closes with 1008; the caller stops there, so nothing is
parsed, no signature is checked, and no storage operation is issued.

The window lives in memory on the Durable Object
(`ConnectionRegistry::admit_message`, using `util::window::WindowCounter`),
not in storage: persisting it would put
a write in front of every frame, and a socket that hibernates has been silent
long enough for a fresh window to be the right answer. A reconnect gets a new
id and a fresh window, so the admission caps — not this budget — are what bound
how often a client can start over; the budget bounds what one connection can
spend while it is here.

Why it exists: one `REQ` re-reads and rewrites the connection's routing index
(up to `SUB_MAX_ROUTING_KEYS` keys), and an `EVENT` pays a Schnorr
check, all inside the one Durable Object every connection of a region shares.
Without a budget, one client could keep that object busy and starve everyone
else. Two smaller rules go with it:

- A `CLOSE` for a subscription the connection does not have returns before
  touching storage (`WalletRegistry::unsubscribe` checks
  `WalletIndex::has_subscription` first), so repeating it costs only the
  budget unit, not a snapshot rewrite.
- A refused frame is not counted, so a client that backs off keeps the rest of
  its window.

Trade-off: the budget is per connection, and a client that reconnects starts a
fresh one. That is delegated to the admission caps rather than adding
per-address counters in storage, which would cost an operation per frame.

## Common Gotchas

1. **Bounded REQs**: every value in a filter (`ids`, `authors`, `kinds`, `#p`, `#e`) becomes a persisted routing key, and `sync()` re-reads and rewrites all of a connection's keys on every subscribe. That is why `Limits::max_filter_items` / `max_routing_keys_per_connection` and the `MAX_SUB_ID_LENGTH` check in `NostrEngine::handle_req` exist; removing them turns one `REQ` into tens of thousands of storage operations.
2. **Deliver to every connection**: a subscription is stored per connection, and a reconnecting client leaves its previous id behind until the old socket reports the close. `WalletRegistry::collect_matches` therefore sends to every recorded connection of a matching subscription — sending only to the newest id lets any later connection silence the others.
3. **DO state**: Use `self.state.storage()`
4. **WebSocket tags**: Use `utils::HibernationState` trait
5. **Panic = Abort**: The project uses `panic = "abort"`. **NEVER** use `unwrap()` or `expect()`. Use `?`, `.get()` for indexing, and checked math to prevent isolate crashes.
6. **`sync()` required for persistence**: Every `subscribe()`/`unsubscribe()` must call `sync()`. The in-memory index is lost on DO hibernation — only `state.storage()` survives. Removing `sync()` breaks event routing after wake. Regression test: `test_hibernation_contract`. `sync()` writes through `put_all`, which splits the entries into puts of at most `MAX_STORAGE_BATCH` (128, the Durable Object limit) and starts them together so they commit as one write. Local workerd does not enforce that limit, so only `MockStorage`, which refuses a larger put, catches a regression. If saving fails, `subscribe` puts the live index back to what storage still holds, because the client is told `CLOSED`.
7. **Routing index families**: A subscription can be indexed under `pk:` (`authors`/`#p`), `eid:` (`ids`) or `etag:` (`#e`); `sync()`, `load_indexed()`, `subscribe()` (when a `REQ` replaces a `sub_id`), `unsubscribe()` and `on_terminate()` must treat all three the same or routing silently degrades to "accepted, EOSE, no event", or the persisted index keeps a membership the live connection no longer has (leaks storage and resurrects dead subscriptions). Cached info events follow the same rule as any other event: they reach a `#p` filter only through a real `p` tag, and only the author key (`info:<author>`) is comparable, so an app's pubkey resolves to nothing.
8. **Alarms are one-shot**: the idle sweep re-arms itself inside `alarm()` and the accept path arms it when none is pending. Dropping either half leaves connections that are accepted once and never swept — the case this sweep exists to close. `CONN_IDLE_TIMEOUT_SECONDS=0` / `CONN_SWEEP_SECONDS=0` switch it off, which is what the functional test suite does so its own sockets are not swept mid-test.
9. **Info cache is bounded on purpose**: `INFO_CACHE_MAX_EVENTS` / `INFO_CACHE_TTL_SECONDS` are the only thing keeping the `info:` family from growing forever, since anyone may publish a `kind` 13194 event. Both are enforced through one paged `list_page("info:", ...)` enumeration, so a new bound must go there rather than into a separate pass. Unit tests set `info_ttl_seconds: 0` (`test_limits()`) because fixtures publish `created_at: 1000` against a pinned clock of 1_700_000_000 — leaving the default TTL on would expire every fixture. Tests that mean to test expiry set the TTL and `set_test_time` explicitly.

## Agent skills

### Issue tracker

GitHub Issues through the GitHub MCP tools, never `gh`. See `docs/agents/issue-tracker.md`.

### Triage labels

The five default labels, each named after its role. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

## Testing Conventions

### Test file layout
- Each source file has a sibling `*_test.rs` wired via `#[cfg(test)] #[path = "..."] mod xxx_test;` — **no `mod tests { }` blocks in source files**.
- The only `#[cfg(test)]` item in a source file is the `mod xxx_test;` declaration. All test code (functions, helpers, impls) lives in the test file.

### Shared test utilities
- `MockStorage`, `simulate_hibernation`, and similar cross-module test helpers live in `src/common/test_helpers.rs`.
- Import with `use crate::common::test_helpers::*;`.

### Async tests
- Use `futures::executor::block_on(async { ... })` inside `#[test]` functions.

### Private field access
- Test files are child modules of the source file they test, so they can access private fields directly.
- Prefer **standalone functions** over inherent methods for test helpers (e.g. `fn new_engine()` instead of `impl NostrEngine<MockStorage> { pub fn new() }`).

### Time mocking
- `TEST_TIME` thread_local, `test_now()`, and `set_test_time()` live in `engine_test.rs`.
- Available to all tests in that file via direct module scope.

### Integration suite (`test/`)
- `test/run-all.js` orchestrates everything; remote runs (`node run-all.js host`)
  skip local-only tests via `isLocal`.
- `test/conformance.test.js` drives the **real `@getalby/sdk`**
  (`NWCClient` / `NWCWalletService`) end-to-end with canned wallet handlers —
  no Lightning node. Keep it aligned with the exact version pinned in
  `test/package.json`; its filters/tags are what Alby Go/Hub send.
  It runs against local and remote relays.
- `test/restart-persistence.test.js` spawns its own `wrangler dev` on port
  8790 with a temp `--persist-to` directory, kills it, restarts on the same
  directory, and asserts the DO storage keys survive (`info:`, `infoid:`,
  delete-by-e-tag). Local only; uses a separate state dir so it never clobbers
  `.wrangler/state`.
- `test/relay-harness.js` (`withRelay`) starts a private `wrangler dev` for
  tests that need non-default limits, and its `QUIET_LIMITS` switch the
  connection gates off unless a scenario turns one on. Each relay runs in its
  own process group, and shutdown signals the group, waits for the exit, and
  returns only once the port is released: `wrangler dev` is npx → node →
  workerd, so a layer that outlives the signal keeps the port bound, and a
  start that cannot bind hangs instead of failing. Scenarios reuse ports, so
  the harness also clears the port before every start.
  `test/connection-limits.test.js` is where those gates are exercised — one
  scenario per gate (address/network caps, idle sweep, slot reclaiming).

## Coverage Gate
- Measure **production code only**: test files (`*_test.rs`) and `cloudflare/`
  are excluded. Test files would otherwise count their own ~99% coverage into
  the denominator; `cloudflare/` needs a live workerd and is covered by the
  integration suite instead.
- Thresholds: **95% lines / 90% regions / 80% functions**
- Run:
  ```
  cargo llvm-cov --locked --ignore-filename-regex '(cloudflare/|_test\.rs$)' \
    --fail-under-lines 95 --fail-under-regions 90 --fail-under-functions 80
  ```

> All worker-dependent modules live under `cloudflare/`. The core relay logic (`nostr/*`, `auth.rs`, `util/*`) has zero `worker` crate dependency and is fully testable.

## Mutation Testing
- Coverage says the code was *executed*; mutation testing proves the tests
  *fail* when behaviour changes. Run it on demand before a release, not in CI.
- Use the wrapper — it picks the right flags for each phase:
  ```
  cargo install cargo-mutants --locked   # once
  ./scripts/mutants.sh            # mutants in lines changed vs main (~2-3 min)
  ./scripts/mutants.sh --iterate  # ... skipping mutants caught last run (~0s)
  ./scripts/mutants.sh --full     # whole configured scope, parallel (release)
  ```
  Two flag facts drive the script: `--in-diff` takes a *diff file*, not a git
  ref, and `-j`/`--jobs` cannot be combined with `--in-place`. Without
  `--in-place` cargo-mutants works on its own copy of the tree — safe for
  parallel jobs and interrupted runs — and the baseline build is ~20s because
  dependency artifacts are cached. Measured here: 411 mutants single-threaded
  with `--in-place` took ~25 min; full scope with `-j 8` took 6 min, and the
  60 diff-only mutants took ~2 min.
- Scope and documented equivalent mutants live in `.cargo/mutants.toml`.
  `exclude_re` patterns are matched against `file:line:col: description`, so
  prefer matching the description over line numbers, which shift.
- `--iterate` matches mutants by `file:line:col: description`; clear
  `mutants.out/` before a release run so a stale result cannot hide a line
  whose behaviour changed.
- The run must end with `0 missed` and no timeouts. Survivors are listed in
  `mutants.out/missed.txt`; fix the test (assert the exact error/edge) rather
  than widening the exclusion.
- If a run is interrupted, `git status` may show a mutated source file (only
  possible when `--in-place` was used); restore it with
  `git checkout -- <file>` before doing anything else.
