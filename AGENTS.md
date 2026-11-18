# Mekhala Agent Context

## Overview
Cloudflare Worker (Rust/WASM) implementing Nostr Wallet Connect (NIP-47) relay with WebSocket Hibernation.

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
- **Modules**: auth.rs, lib.rs, cloudflare/, lnaddress/, nostr/, util/

## Critical Dependencies
- `worker` 0.8.x runtime
- `k256` for Schnorr signatures

## Required Env Vars
- `RELAY_SECRET` - password (set via Cloudflare dashboard)
- `WALLET_REGION` - optional: apac/weur/wnam (default: apac)
- `MAX_CONNECTIONS` - optional, default: 100
- `MAX_CONNECTIONS_PER_IP` - optional, default: 5, `0` disables (see
  "Connection admission" below)
- `MAX_CONNECTIONS_PER_ASN` - optional, default: 20, `0` disables
- `IDLE_TIMEOUT_SECONDS` - optional, default: 120, `0` disables the sweep
  (connections idle longer than this *and* holding no subscription are closed)
- `CONNECTION_SWEEP_SECONDS` - optional, default: 60, `0` disables the sweep
- `EVICT_MIN_IDLE_SECONDS` - optional, default: 10: minimum idle time before a
  subscription-less connection may be closed to free a slot
- `MAX_SUBSCRIPTIONS_PER_CONNECTION` - optional, default: 100
- `MAX_FILTER_ITEMS` - optional, default: 10 (max values per filter field)
- `MAX_EVENT_TAGS` - optional, default: 10
- `MAX_ROUTING_KEYS_PER_CONNECTION` - optional, default: 1000
- `MAX_INFO_EVENTS` - optional, default: 1000, `0` disables the cap (see
  "Info cache" below)
- `INFO_TTL_SECONDS` - optional, default: 2592000 (30 days), `0` disables
  expiry
- `MIN_AMOUNT_MSAT` - optional, default: 1000: lowest LN address invoice
  amount, advertised and enforced
- `MAX_AMOUNT_MSAT` - optional, default: 100000000: highest LN address
  invoice amount, advertised and enforced
- `CALLBACK_RATE_LIMIT_PER_MINUTE` - optional, default: 10, `0` disables (see
  "LN address callbacks" below)
- `RATE_LIMIT_MAX_KEYS` - optional, default: 1000, `0` removes the cap: how many
  sources may own a counter before new ones share one
- `LN_ADDRESS_KEY` - required to use Lightning Addresses: 64 hex characters
  (32 bytes). Seals stored wallet connections; without a usable value no
  Lightning Address is served or stored (see "LN address storage" below)
- `LN_ADDRESS_KEY_PREVIOUS` - optional: the previous `LN_ADDRESS_KEY` during a
  rotation, kept readable until every record has been re-sealed
- `MAX_CONTENT_LENGTH` - default: 98304 (96 KB, covers the 87,472-character NIP-44 ceiling)

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

1. `MAX_CONNECTIONS_PER_IP` — connections sharing one address tag.
2. `MAX_CONNECTIONS_PER_ASN` — connections sharing one autonomous system.
3. `MAX_CONNECTIONS` — the relay total. On reaching it, the longest-idle
   connection **that holds no subscription** is closed to free a slot (only
   when it has been idle at least `EVICT_MIN_IDLE_SECONDS`); with no such
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
timestamp is refreshed at most every 30 s. `IDLE_TIMEOUT_SECONDS` closes
connections that hold no subscription and have been silent that long, driven
by the Durable Object alarm (`alarm()`), which re-arms itself every
`CONNECTION_SWEEP_SECONDS` while sockets exist.

Trade-offs: per-address caps put CGNAT and corporate NAT users in one bucket,
so keep them generous (or disable them) where that matters; a wallet that is
merely idle keeps its subscription and is never swept.

## Info cache

`kind` 13194 events are cached so a hibernated or freshly connected client can
still answer a wallet-discovery `REQ`. Each cached event occupies exactly two
keys, `info:<author>` (the event) and `infoid:<event id>` (reverse mapping), and
the cache is bounded by two independent settings:

- `MAX_INFO_EVENTS` caps how many events are kept. Enforced on publish
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
- `INFO_TTL_SECONDS` expires events, measured from the event's own
  `created_at`. Enforced on read (`get_info` treats an expired event as absent
  and deletes it) and on the Durable Object alarm, so a cache nobody reads
  still shrinks.

Both bounds run through `WalletRegistry::trim_info_cache`, which needs an
enumeration — `Storage::list_prefix("info:")` — because the number of cached
wallets is not tracked anywhere that survives hibernation. `0` disables either
bound; setting both to `0` removes the enumeration entirely.

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
`prune_legacy_info_aliases()`, which the alarm calls. Nothing writes them any
more, so the sweep is a one-way migration and its "already empty" case is a
single `list_prefix`.

## LN address callbacks

`GET /lnaddress/:username/callback?amount=<msat>` passes four gates, in this
order, before it reaches a wallet (`src/cloudflare/handler.rs`):

1. **Amount present** — a missing or non-numeric `amount` is refused without
   touching anything else.
2. **Amount in range** — `MIN_AMOUNT_MSAT..=MAX_AMOUNT_MSAT`. The `.well-known`
   document advertises these same values as `minSendable`/`maxSendable`, so a
   client is never told one range and refused for another. A zero bound or an
   inverted range cannot be advertised meaningfully, so it falls back to the
   defaults and logs once; the range is never switched off.
3. **Per-source rate limit** — `CALLBACK_RATE_LIMIT_PER_MINUTE` callbacks per
   60-second window, keyed on the address tag the Worker stamps on the
   forwarded request (`x-mekhala-client-ip-tag`). A request with no tag (local
   development, or an edge that reports no address) is admitted, matching
   per-address connection limits. Refused requests get HTTP 200 with
   `{"status":"ERROR","reason":"Too many requests, retry in Ns"}` and do not
   consume a slot or write storage.
4. **Store lookup and invoice** — only now is the username resolved and the
   wallet asked to mint an invoice.

Each source owns a counter at `rate:<ip-tag>`, but the family is capped by
`RATE_LIMIT_MAX_KEYS`: once that many sources hold a key, a source that has
none is charged to `rate:overflow` instead. Without the cap, rotating through
addresses would let one caller grow Durable Object storage without bound. An
existing counter always keeps working, so the cap limits who may open a new
one, not how much traffic an owner may send. The count is held in memory per
isolate, seeded by one listing, and corrected by every sweep.

Invoice failures answer with one fixed reason (`Wallet unavailable`) and the
wallet's own message goes to the log. The callback endpoint is anonymous, and
its errors would otherwise tell a caller whether the wallet is online, short of
funds, or rejecting the amount.

The counters live in Durable Object storage under `rate:<ip-tag>` and are
swept by the same alarm as the idle and info sweeps, with the pass delay taken
from `CONNECTION_SWEEP_SECONDS` (independent of `IDLE_TIMEOUT_SECONDS`: a relay
may keep sockets forever and still want counters cleaned). Each pass deletes at
most 128 keys because `delete_batch` issues one storage call per key, and the
alarm re-arms itself while any counter remains, so the family drains after
traffic stops even with no sockets. Setting `CONNECTION_SWEEP_SECONDS=0` turns
sweeping off entirely, and then `rate:` keys linger until read again.

## LN address storage

A stored NWC URI can spend from the user's wallet, so it is never kept in
plaintext. `CloudflareKvStore` (`src/cloudflare/kv.rs`) stores each registration
under `lnaddr:<username>` as `v1:<NIP-44 payload>`, sealed with a key derived
from `LN_ADDRESS_KEY` by HKDF-SHA256 with info `lnaddr:v1`
(`src/lnaddress/record.rs`). The sealed JSON carries the version, the username,
the URI, and the verified methods once a registration has checked them.

The username is sealed inside the payload and compared after opening, so a row
copied to another username's key decrypts but is refused — moving KV rows
around cannot attach one user's wallet to another address. `Debug` for the
record and for the key set prints no URI or key material, because both end up
in log lines on the error paths.

Behavior that matters operationally:

- **Missing or malformed key fails closed.** Sealed rows cannot be opened, and
  plaintext rows from earlier versions are refused rather than served, so a
  forgotten secret cannot silently downgrade storage. No row is deleted in that
  state; fix the secret and the records work again.
- **Plaintext rows migrate lazily.** Earlier versions stored the URI under the
  bare username. The first read seals the row under `lnaddr:<username>` and
  then deletes the plaintext key (sealed write first, so an interruption cannot
  lose the URI). There is no separate migration step or version bump.
- **Rotation is two-step.** Set `LN_ADDRESS_KEY_PREVIOUS` to the old key and
  `LN_ADDRESS_KEY` to the new one: reads try the current key first, then the
  previous one. Only rows written again are sealed under the new key, so keep
  `LN_ADDRESS_KEY_PREVIOUS` set until every record has been rewritten.
- **Usernames are bounded.** At most 64 characters of `[A-Za-z0-9._~-]`, and
  `.`/`..` are refused: those two are the only path segments an HTTP client
  rewrites, which would turn a callback URL into one that misses this route.
  The same check guards `.well-known`, callbacks, and stored registrations.
- **Local development needs the key too.** `wrangler dev` without `LN_ADDRESS_KEY`
  (in `.dev.vars` or as `--var LN_ADDRESS_KEY:<64 hex chars>`) answers every
  Lightning Address lookup with `User not found`; the integration suite passes
  one for exactly this reason.

## Common Gotchas

1. **Bounded REQs**: every value in a filter (`ids`, `authors`, `kinds`, `#p`, `#e`) becomes a persisted routing key, and `sync()` re-reads and rewrites all of a connection's keys on every subscribe. That is why `Limits::max_filter_items` / `max_routing_keys_per_connection` and the `MAX_SUB_ID_LENGTH` check in `NostrEngine::handle_req` exist; removing them turns one `REQ` into tens of thousands of storage operations.
2. **Deliver to every connection**: a subscription is stored per connection, and a reconnecting client leaves its previous id behind until the old socket reports the close. `WalletRegistry::collect_matches` therefore sends to every recorded connection of a matching subscription — sending only to the newest id lets any later connection silence the others.
3. **DO state**: Use `self.state.storage()`
4. **WebSocket tags**: Use `utils::HibernationState` trait
5. **Panic = Abort**: The project uses `panic = "abort"`. **NEVER** use `unwrap()` or `expect()`. Use `?`, `.get()` for indexing, and checked math to prevent isolate crashes.
6. **`sync()` required for persistence**: Every `subscribe()`/`unsubscribe()` must call `sync()`. The in-memory index is lost on DO hibernation — only `state.storage()` survives. Removing `sync()` breaks event routing after wake. Regression test: `test_hibernation_contract`.
7. **Routing index families**: A subscription can be indexed under `pk:` (`authors`/`#p`), `eid:` (`ids`) or `etag:` (`#e`); `sync()`, `load_indexed()`, `unsubscribe()` and `on_terminate()` must treat all three the same or routing silently degrades to "accepted, EOSE, no event", or the persisted index keeps a membership the live connection no longer has (leaks storage and resurrects dead subscriptions). Cached info events follow the same rule as any other event: they reach a `#p` filter only through a real `p` tag, and only the author key (`info:<author>`) is comparable, so an app's pubkey resolves to nothing.
8. **Alarms are one-shot**: the idle sweep re-arms itself inside `alarm()` and the accept path arms it when none is pending. Dropping either half leaves connections that are accepted once and never swept — the case this sweep exists to close. `IDLE_TIMEOUT_SECONDS=0` / `CONNECTION_SWEEP_SECONDS=0` switch it off, which is what the functional test suite does so its own sockets are not swept mid-test.
9. **Info cache is bounded on purpose**: `MAX_INFO_EVENTS` / `INFO_TTL_SECONDS` are the only thing keeping the `info:` family from growing forever, since anyone may publish a `kind` 13194 event. Both are enforced through one `list_prefix("info:")` enumeration, so a new bound must go there rather than into a separate pass. Unit tests set `info_ttl_seconds: 0` (`test_limits()`) because fixtures publish `created_at: 1000` against a pinned clock of 1_700_000_000 — leaving the default TTL on would expire every fixture. Tests that mean to test expiry set the TTL and `set_test_time` explicitly.
10. **LN address range is one value**: the `.well-known` advertisement and the callback check both read `SendableLimits` from `CloudflareConfig::lnaddress_policy()`. Changing one side alone makes the service advertise a range it refuses. The callback rate limiter is only swept while `CONNECTION_SWEEP_SECONDS` is non-zero; that switch is deliberately separate from `IDLE_TIMEOUT_SECONDS`.

## Agent skills

### Issue tracker

GitHub Issues via GitHub MCP. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout. See `docs/agents/domain.md`.

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
  connection gates off unless a scenario turns one on.
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

> All worker-dependent modules live under `cloudflare/`. The core relay logic (`nostr/*`, `auth.rs`, `lnaddress/*`) has zero `worker` crate dependency and is fully testable.

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
