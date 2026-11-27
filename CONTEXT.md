# Mekhala Domain Context

A high-performance, stateless Nostr relay optimized for **NIP-47 (Nostr Wallet Connect)** on Cloudflare Workers.

## Core Concepts

### Nostr Wallet Connect (NWC)
A protocol (NIP-47) that allows applications to communicate with Lightning wallets via Nostr. It uses specific event kinds for requests, responses, and notifications.

### Relay
The ephemeral routing engine. Mekhala is a specialized relay that only processes NWC-related events and does not persist them.

### WebSocket Hibernation
A Cloudflare Workers feature where Durable Objects can release memory while keeping a WebSocket connection "alive" in a low-power state. Mekhala uses this to maintain NWC subscriptions efficiently.

### Connection State
The data attached to a WebSocket connection, including active subscriptions and security limits. This state is serialized and restored during hibernation/wake-up.

### Out of scope: wallet-side work
Mekhala never decrypts NIP-47 content, never sends NWC requests of its own, and stores no wallet connection strings. Lightning Address (LNURL-pay) and anything else that needs a wallet's secret runs in a separate service that connects to the relay as an ordinary NWC client.

## Domain Language

- **NWC Event**: A Nostr event with a kind in the 13194 or 23194-23197 range.
- **Filter**: A Nostr subscription filter (NIP-01).
- **Subscription**: A persistent request for events matching specific filters.
- **Relay Secret**: An optional path parameter used to restrict access to the relay.
- **Hibernation Tag**: The metadata (serialized `ConnectionState`) attached to a hibernating WebSocket.
- **SavedState**: A structured object containing a connection's serialized state (for persistence) and the routing keys referenced by its filters — pubkeys (`authors`/`#p`), event ids (`ids`), and `#e` tags — used to maintain a clean boundary between domain logic and transport-level storage.
- **WalletRegistry**: A deep module that manages NWC subscription indexing and event routing. It encapsulates the subscription index (filters → connection IDs), the routing index families (pubkey → subscriptions + info event, event id → subscriptions, `#e` tag → subscriptions), and coordinates with an asynchronous storage layer to persist and restore connection state.
- **Routing index family**: One of the persisted key spaces that map an event attribute to the connections interested in it: `pk:` (`authors`, `#p`), `eid:` (`ids`) and `etag:` (`#e`). All three are written by `sync()`, read lazily by `load_indexed()` when an event arrives, and cleaned by `on_terminate()`. A subscription whose filter only references an event id still routes correctly, including after hibernation.
- **Info cache** (`info:<author>`, `infoid:<event id>`): The two keys a cached `13194` event occupies. Lookup is by author only: a `p` tag on an info event names the app the wallet announced itself to and is not indexed, so `{kinds:[13194], "#p":[<app>]}` replays nothing. The wallet-auth / one-click Nostr discovery flow is out of scope.
- **Clock seam**: `NostrEngine` accepts `fn() -> u64` as a clock parameter, `crate::util::now` in production. No domain module calls `now()` directly — all timestamps flow through the injected seam.
- **WindowCounter** (`src/util/window.rs`): A fixed 60-second admission window. The Durable Object keeps one per connection, in memory, to budget inbound WebSocket frames (`CONN_MAX_MESSAGES_PER_MINUTE`); a refused frame consumes no slot, and a limit of `0` admits everything.
- **Retired key families** (`info-p:`, `rate:`): Storage keys earlier versions wrote and nothing current reads. `WalletRegistry::prune_retired_keys` deletes them on the alarm, at most 128 per pass across all families.

## Architecture Map

### 1. Transport Layer (The Edge)
**Module:** `CloudflareTransport` (`src/cloudflare/transport.rs`)
- **Role:** The entry point living on Cloudflare's edge. It manages the Durable Object lifecycle, physical WebSockets, and **WebSocket Hibernation**. It translates raw incoming HTTP/WebSocket traffic into domain events.
- **Provides:** `CloudflareStorage`, a concrete adapter for the `Storage` trait, interacting with the Durable Object's internal KV.
- **Composed of:**
  - `ConnectionRegistry`: manages physical WebSocket connections, hibernation recovery and the per-connection message budget.
- **Calls Down To:**
  - `NostrEngine` to process incoming messages.
  - `WalletRegistry` (via the engine) to proactively wake up hibernating WebSockets when an incoming event targets an offline **Subscription**.

### 2. The Core Domain (The Routing Engine)
**Module:** `NostrEngine` (`src/nostr/engine.rs`)
- **Role:** The stateless **Relay** brain. It is completely decoupled from Cloudflare and networking. It validates incoming NIP-01/NIP-47 messages, enforces security limits, verifies signatures, and determines which connections should receive an event based on active filters.
- **Seams:** `clock: fn() -> u64` (injected for deterministic timestamp validation), `Storage` trait (injected for async persistence).
- **Calls Down To:**
  - Protocol parsers (`Event`, `Filter`, `ClientMessage`).
  - `WalletRegistry` to find the recipients for a verified **NWC Event**.

### 3. State & Indexing Layer
**Module:** `WalletRegistry` (`src/nostr/wallet_registry.rs`)
- **Role:** A deep module that manages the state of the relay. It encapsulates two major concerns:
  1.  **Asynchronous Persistence:** It takes the `Storage` trait and automatically synchronizes **SavedState** (subscriptions and cached info events) to disk when changes occur. It also handles hydrating state back into memory when an offline user receives an event.
  2.  **Synchronous Indexing (`WalletIndex`):** An internal, pure data structure containing the `HashMap`s needed to quickly resolve a pubkey or event tag to a specific connection ID.
- **Called By:** `NostrEngine`.

### The Data Flow
1. A physical WebSocket wakes up from **WebSocket Hibernation** and sends text to `CloudflareTransport`.
2. `CloudflareTransport` asks `NostrEngine` to handle the text.
3. `NostrEngine` parses it into an **NWC Event** or **Filter** and validates it.
4. If it's a **Subscription** request, the engine tells `WalletRegistry` to subscribe.
5. `WalletRegistry` updates its pure in-memory `WalletIndex` and async-flushes a **SavedState** via `Storage`.
6. If it's an **NWC Event** to be broadcast, the engine asks `WalletRegistry` for matching connection IDs.
7. `NostrEngine` returns an array of `EngineResponse::Send` commands back up to `CloudflareTransport`.
8. `CloudflareTransport` delivers the bytes to the physical WebSockets (via `ConnectionRegistry`).
