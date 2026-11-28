# Mekhala glossary

The project's domain language and architecture map. Engineering skills read
this file before working on the domain model; see `docs/agents/domain.md` for
how they use it, and `docs/adr/` for past decisions. Operational rules
(commands, limits, gotchas, testing conventions) live in `AGENTS.md`.

## What Mekhala is

A Nostr relay on Cloudflare Workers, specialized for **NIP-47 (Nostr Wallet
Connect)**. It validates events and routes them to subscribers. It keeps no
event history, never decrypts NIP-47 content, never sends NWC requests of its
own, and stores no wallet connection strings. Lightning Address (LNURL-pay) and
anything else that needs a wallet's secret runs in a separate service that
connects to the relay as an ordinary NWC client.

## Domain language

- **NWC event**: a Nostr event of kind `13194` (wallet info) or `23194`–`23197`
  (request, response, notifications). Kind `5` is also accepted, to delete a
  cached info event.
- **Filter**: a NIP-01 subscription filter.
- **Subscription**: a filter set registered by a connection under a `sub_id`.
  A `REQ` with an existing `sub_id` replaces it.
- **Relay secret** (`AUTH_RELAY_SECRET`): the path segment that admits a
  client (`wss://host/<secret>`). Without one the relay is public.
- **Attachment**: what rides on a hibernatable WebSocket: connection id,
  ip-tag (a hash of the address), ASN and last-seen time. Subscriptions are not
  in it; they are in storage.
- **SavedState**: a connection's subscriptions serialized for `conn:<id>`, plus
  the routing keys its filters reference: pubkeys (`authors`, `#p`), event ids
  (`ids`) and `#e` tags.
- **Routing index family**: a persisted key space that maps an event attribute
  to the connections interested in it: `pk:` (`authors`, `#p`), `eid:` (`ids`)
  and `etag:` (`#e`). `sync()` adds memberships; `subscribe` (on replace),
  `unsubscribe`, `on_terminate` and the orphan sweep remove them; `load_indexed`
  reads them when an event arrives and drops ids whose `conn:` is gone.
- **Info cache** (`info:<author>`, `infoid:<event id>`): the two keys a cached
  `13194` event occupies, each event at most 4 KiB. Kind `13194` is
  replaceable, so only a newer event (later `created_at`, or the lower id on a
  tie) replaces the cached one. Lookup is by author only:
  a `p` tag on an info event is not indexed, so `{kinds:[13194], "#p":[<app>]}`
  replays nothing. The wallet-auth / one-click discovery flow is out of scope.
- **Orphaned connection**: stored state (`conn:` and routing memberships) whose
  socket was dropped without a close event, by a deploy, an eviction or a
  crash. Removed on the next event that targets it, or by the alarm.
- **Retired key families** (`info-p:`, `rate:`): keys earlier versions wrote and
  nothing current reads, deleted by the alarm at most 128 per pass.
- **Clock seam**: `NostrEngine` takes `fn() -> u64` as its clock
  (`crate::util::now` in production). No domain module calls `now()` directly.
- **WindowCounter** (`src/util/window.rs`): a fixed 60-second window. The
  Durable Object keeps one per connection, in memory, to budget inbound frames
  (`CONN_MAX_MESSAGES_PER_MINUTE`); a refused frame consumes no slot, and a
  limit of `0` admits everything.

## Architecture map

### 1. Edge Worker
`src/lib.rs`, `src/cloudflare/router.rs`, `src/cloudflare/identity.rs`

Stateless and runs in every colo. It checks the relay secret (constant time),
answers NIP-11 on a plain `GET`, hashes the client address into an ip-tag and
reads the ASN, and forwards the WebSocket upgrade to the one Durable Object
named `GLOBAL` (placed with `DO_REGION`).

### 2. Transport (Durable Object)
`CloudflareTransport` (`src/cloudflare/transport.rs`)

Owns the sockets, WebSocket Hibernation and the alarm. It provides
`CloudflareStorage`, the `Storage` adapter over Durable Object storage.

- `ConnectionRegistry` (`src/cloudflare/connection.rs`): sockets, admission
  (per-address, per-ASN and total caps) and the per-connection message budget.
- The alarm runs the idle sweep, the info cache trim, the retired-key sweep and
  the orphan sweep.
- Calls `NostrEngine` for every frame and carries out its responses: `Send` to
  deliver, `WakeUp` to restore a hibernated subscriber (or remove it when its
  socket is gone).

### 3. Engine
`NostrEngine` (`src/nostr/engine.rs`)

Decoupled from Cloudflare. It validates events (kind, size, time, id,
signature), handles `REQ`/`CLOSE` and enforces the filter limits, caches info
events and applies NIP-09 deletions, and asks `WalletRegistry` who receives an
event.

### 4. State and indexing
`WalletRegistry` (`src/nostr/wallet_registry.rs`)

The deep module that owns relay state:

1. **Persistence** through the `Storage` trait: `get`, `put_batch` (at most
   128 entries per call; `put_all` splits larger writes), `delete_batch` and
   `list_page`. It saves `SavedState` on every subscribe and restores it when an
   event targets a hibernated connection.
2. **Indexing** (`WalletIndex`): pure in-memory maps from pubkey, event id and
   `#e` tag to subscriptions, rebuilt from storage on demand.

## Data flow

1. A client's frame reaches `CloudflareTransport`, waking the Durable Object if
   it was hibernating; the frame is charged against the connection's budget.
2. `NostrEngine` parses it as an `EVENT`, `REQ` or `CLOSE`.
3. A `REQ` subscribes through `WalletRegistry`, which updates `WalletIndex`
   and saves the `SavedState`; a failed save rolls the index back and answers
   `CLOSED`.
4. An `EVENT` is validated and answered with `OK`; `WalletRegistry` then looks
   up its routing keys, restores any hibernated subscriber from storage, and
   returns the matching connections.
5. The engine returns `EngineResponse::Send` and `WakeUp` commands, and the
   transport delivers them through `ConnectionRegistry`.
