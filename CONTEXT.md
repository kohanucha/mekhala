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

### LN Address Bridging
Mekhala provides an LN Address bridge, allowing users to pay to an LN Address (e.g., `user@relay.com`) which then triggers an NWC request to the user's wallet.

## Domain Language

- **NWC Event**: A Nostr event with a kind in the 13194 or 23194-23197 range.
- **Filter**: A Nostr subscription filter (NIP-01).
- **Subscription**: A persistent request for events matching specific filters.
- **Relay Secret**: An optional path parameter used to restrict access to the relay.
- **Hibernation Tag**: The metadata (serialized `ConnectionState`) attached to a hibernating WebSocket.
- **SavedState**: A structured object containing a connection's serialized state (for persistence) and an explicit set of associated pubkeys (for indexing). Used to maintain a clean boundary between domain logic and transport-level storage.
- **WalletRegistry**: A deep module that manages NWC subscription indexing and event routing. It encapsulates the subscription index (filters → connection IDs), pubkey index (pubkey → subscriptions + info event), and coordinates with an asynchronous storage layer to persist and restore connection state.
- **InternalConnection**: A connection tracked by WalletRegistry that has subscriptions but no backing WebSocket. Used by the LN Address bridge and RPC flow to receive routed NWC responses via oneshot channels instead of WebSocket delivery.
- **NwcRpcOrchestrator**: The pure coordination logic that drives an NWC RPC request-response cycle. It uses `NwcRpcMachine` for state transitions and delegates I/O to an `RpcContext` trait — making the full RPC flow unit-testable without Cloudflare Workers.
- **RpcContext**: A 5-method `?Send` trait (clock, ID allocation, action dispatch, response receipt, cleanup) that `CloudflareTransport` implements. The orchestrator calls through this seam; test doubles implement it with pre-configured responses.
- **RpcReceiveError**: The error type for the `receive_response` seam — `Timeout` or `ChannelClosed`, both representable without Cloudflare-specific types.
- **Clock seam**: Both `NostrEngine` and `NwcClient` accept `fn() -> u64` as a clock parameter, defaulting to `crate::util::now`. No domain module calls `now()` directly — all timestamps flow through the injected seam.
- **UserStore**: A `?Send` trait for looking up NWC connection URIs by username. `CloudflareKvStore` implements it using Workers KV. The seam enables unit-testing the LN Address handler without a worker runtime.
- **ConnectionManager**: A deep module that manages connection dispatch and lifecycle. It routes `EngineResponse` to WebSocket peers or internal RPC channels, orchestrates hibernation wake-up via `ConnectionTransport`, and calls back to `ConnectionHandler` for engine state persistence. Fully unit-testable without Cloudflare Workers.
- **ConnectionTransport**: The seam abstracting peer I/O and hibernation activation. Production adapter (`CloudflareConnectionTransport`) wraps `WebSocketRegistry` using `js_sys::Object::is` for identity. Test adapter (`MockTransport`) uses in-memory tracking.
- **ConnectionHandler**: The seam abstracting engine callbacks for connection lifecycle events (`on_connect`, `load`, `on_terminate`). Production adapter (`NostrEngineHandler`) wraps `NostrEngine`. Test adapter (`MockHandler`) records calls.
- **CloudflareConnectionTransport**: The production adapter for `ConnectionTransport`. Wraps `WebSocketRegistry` and holds `State` for hibernation recovery.
- **LnAddressHandler<S>**: A struct generic over `UserStore` that orchestrates LNURL-pay requests and callbacks. It resolves usernames to NWC URIs via the `UserStore` seam and delegates invoice creation to `LnAddressGateway` and `NwcSession` via the `NwcTransport` seam.

## Architecture Map

### 1. Transport Layer (The Edge)
**Module:** `CloudflareTransport` (`src/cloudflare/transport.rs`)
- **Role:** The entry point living on Cloudflare's edge. It manages the Durable Object lifecycle and translates raw incoming HTTP/WebSocket traffic into domain events.
- **Provides:** `CloudflareStorage`, a concrete adapter for the `Storage` trait, interacting with the Durable Object's internal KV.
- **Composed of:**
  - `ConnectionManager<CloudflareConnectionTransport>`: manages dispatch, hibernation wake-up, and internal channels.
  - `CloudflareConnectionTransport`: production adapter for `ConnectionTransport`, wrapping `WebSocketRegistry`.
- **Calls Down To:**
  - `NostrEngine` to process incoming messages.
  - `ConnectionManager` to dispatch `EngineResponse` and manage connection lifecycle.
  - `WalletRegistry` (via the engine) to proactively wake up hibernating WebSockets when an incoming event targets an offline **Subscription**.

### 1a. Connection Management Layer
**Module:** `ConnectionManager` (`src/nostr/connection.rs`)
- **Role:** A deep module that owns connection dispatch and lifecycle. It routes `EngineResponse::Send` to peers or internal channels, handles `EngineResponse::WakeUp` by activating hibernated peers, and delegates engine callbacks to `ConnectionHandler`.
- **Seams:** `ConnectionTransport` trait (peer I/O and activation), `ConnectionHandler` trait (engine callbacks for load/terminate/connect).
- **Production Adapters:** `CloudflareConnectionTransport` (WebSocketRegistry + State), `NostrEngineHandler` (MutexGuard<NostrEngine>).
- **Test Doubles:** `MockTransport` (in-memory tracking), `MockHandler` (call recording).
- **Called By:** `CloudflareTransport` for all WebSocket lifecycle and dispatch operations.

### 2. The Core Domain (The Routing Engine)
**Module:** `NostrEngine` (`src/nostr/engine.rs`)
- **Role:** The stateless **Relay** brain. It is completely decoupled from Cloudflare and networking. It validates incoming NIP-01/NIP-47 messages, enforces security limits, verifies signatures, and determines which connections should receive an event based on active filters.
- **Seams:** `clock: fn() -> u64` (injected for deterministic timestamp validation), `Storage` trait (injected for async persistence).
- **Calls Down To:**
  - Protocol parsers (`Event`, `Filter`, `ClientMessage`).
  - `WalletRegistry` to find the recipients for a verified **NWC Event**.

### 2a. NWC Client Layer
**Module:** `NwcClient` (`src/nostr/nip_47.rs`)
- **Role:** Creates and parses NWC request/response events (encryption, signing, expiration tags).
- **Seams:** `clock: fn() -> u64` (injected for deterministic event creation and verification timestamps).
- **Production adapter:** `crate::util::now` (default in `NwcClient::new`).
- **Test double:** `test_now` / custom clock function.

### 2b. RPC Orchestration Layer
**Module:** `NwcRpcOrchestrator` (`src/nostr/rpc_orchestrator.rs`)
- **Role:** Drives the full NWC RPC request-response lifecycle using `NwcRpcMachine` for state transitions. Delegates all I/O to the `RpcContext` trait, making it fully unit-testable.
- **Seams:** `RpcContext` trait — `now()`, `allocate_connection_id()`, `execute_action()`, `receive_response()`, `disconnect()`.
- **Production Adapter:** `CloudflareTransport` implements `RpcContext` (maps to oneshot channels + Delay timers + engine locks).
- **Test Double:** `MockRpcContext` (pre-configured response queue + action recording).

### 3. State & Indexing Layer
**Module:** `WalletRegistry` (`src/nostr/wallet_registry.rs`)
- **Role:** A deep module that manages the state of the relay. It encapsulates two major concerns:
  1.  **Asynchronous Persistence:** It takes the `Storage` trait and automatically synchronizes **SavedState** (subscriptions and cached info events) to disk when changes occur. It also handles hydrating state back into memory when an offline user receives an event.
  2.  **Synchronous Indexing (`WalletIndex`):** An internal, pure data structure containing the `HashMap`s needed to quickly resolve a pubkey or event tag to a specific connection ID.
- **Called By:** `NostrEngine`.

### 4. Integration Layer (The Bridge)
**Module:** `LnAddressHandler<S: UserStore>` (`src/lnaddress/handler.rs`)
- **Role:** Orchestrates LNURL-pay requests and callbacks using domain logic (`LnAddressGateway`, `NwcSession`) accessed through seams (`UserStore`, `NwcTransport`).
- **Seams:** `UserStore` (username → NWC URI lookup), `NwcTransport` (existing, for RPC).
- **Production Adapter:** `CloudflareKvStore` wraps `worker::KvStore` for Workers KV.
- **Test Double:** `MockUserStore` (HashMap-backed).

### 4b. LN Address Domain Logic
**Module:** `LnAddressGateway` (`src/lnaddress/gateway.rs`)
- **Role:** Pure domain logic for LNURL-pay metadata generation, callback URL construction, and description hashing. No infrastructure dependencies.
- **Called By:** `LnAddressHandler`.

### The Data Flow
1. A physical WebSocket wakes up from **WebSocket Hibernation** and sends text to `CloudflareTransport`.
2. `CloudflareTransport` identifies the WebSocket via `ConnectionManager` and loads its state from `WalletRegistry`.
3. `CloudflareTransport` asks `NostrEngine` to handle the text.
4. `NostrEngine` parses it into an **NWC Event** or **Filter** and validates it.
5. If it's a **Subscription** request, the engine tells `WalletRegistry` to subscribe.
6. `WalletRegistry` updates its pure in-memory `WalletIndex` and async-flushes a **SavedState** via `Storage`.
7. If it's an **NWC Event** to be broadcast, the engine asks `WalletRegistry` for matching connection IDs.
8. `NostrEngine` returns an array of `EngineResponse::Send` and `EngineResponse::WakeUp` commands back up to `CloudflareTransport`.
9. `CloudflareTransport` delegates dispatch to `ConnectionManager`, which routes to physical WebSockets (via `ConnectionTransport`) or internal RPC callers (via `InternalConnectionMap`).

### The RPC Flow (LN Address Bridge / programmatic NWC)
1. An HTTP callback arrives for LN Address bridging (or a programmatic NWC call is made).
2. The caller invokes `NwcTransport::execute_nwc_rpc`.
3. This delegates to `NwcRpcOrchestrator::execute_nwc_rpc`, which drives the full lifecycle via the `RpcContext` seam.
4. The orchestrator allocates an InternalConnection, starts `NwcRpcMachine` (subscribe + publish), and waits for a wallet response.
5. When the wallet's response arrives via WebSocket, `ConnectionManager::route_send` delivers it to the `InternalConnectionMap` oneshot channel.
6. The orchestrator picks up the response, transitions the machine, and returns the result event (or times out / fails).
7. The orchestrator disconnects the InternalConnection and cleans up.
