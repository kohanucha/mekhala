# Specification: nwc-edge-relay (Public Stateless NWC Relay)

## 1. Project Overview
A specialized, high-performance, and stateless Nostr relay built with **Rust** for **Cloudflare Workers**, named **`nwc-edge-relay`**. The primary goal is to provide a public, zero-maintenance bridge for **NIP-47 (Nostr Wallet Connect)**. It operates entirely in-memory without persistent storage, routing NWC events freely, securely, and efficiently.

## 2. Technical Stack
* **Language:** Rust (Stable)
* **Target:** WebAssembly (`wasm32-unknown-unknown`)
* **Framework:** `worker-rs` (Cloudflare Workers Rust SDK)
* **Nostr Protocol:** `nostr` core crate (Wasm-compatible)
* **Deployment:** Cloudflare Workers (using `wrangler` CLI)
* **CI/CD:** GitHub Actions
* **Version Control:** Git (Remote: `git@github.com:kohanucha/nwc-edge-relay.git`)

## 3. Pre-Implementation Instructions for AI Agent
**CRITICAL:** This specification requires strict adherence to the Nostr protocol. Base your implementation on these exact standards:
* [NIP-01: Basic protocol](https://github.com/nostr-protocol/nips/blob/master/01.md)
* [NIP-11: Relay Information Document](https://github.com/nostr-protocol/nips/blob/master/11.md)
* [NIP-47: Nostr Wallet Connect](https://github.com/nostr-protocol/nips/blob/master/47.md)

## 4. Coding Philosophy
* **Robust & Verifiable:** Write testable functions. Separate pure logic (event validation, filter matching) from I/O bounds (Cloudflare worker context).
* **Stateless Performance:** Optimize for zero-latency event broadcasting.
* **Fail Fast:** If an event is malformed or signatures are invalid, drop it immediately and return an appropriate `NOTICE` or `OK(false)` message.

---

## 5. Detailed Protocol Requirements (NIPs)

### A. NIP-01: Basic Protocol (Stateless Implementation)
The relay must handle WebSocket connections and standard Nostr JSON arrays.
* **Client to Relay Messages:**
    * `["EVENT", <event_json>]`: 
        * Must compute the `id` (SHA256 of the serialized event) and verify it matches the provided `id`.
        * Must verify the `sig` (Schnorr signature) against the `pubkey`.
        * If valid, respond with `["OK", <event_id>, true, ""]` and broadcast to matching subscribers.
        * If invalid, respond with `["OK", <event_id>, false, "invalid: signature verification failed"]`.
    * `["REQ", <subscription_id>, <filters_json>...]`: 
        * Store the subscription and filters in memory linked to the WebSocket connection.
        * **Stateless behavior:** Immediately send `["EOSE", <subscription_id>]` (End of Stored Events) since there is no database to query historical events.
    * `["CLOSE", <subscription_id>]`: Remove the specific subscription from memory.
* **Unsupported Messages:** If `AUTH` or `COUNT` are received, ignore them or return a `NOTICE` stating they are unsupported.

### B. NIP-11: Relay Information Document
Provide metadata to clients to announce NWC compatibility.
* **Endpoint:** HTTP `GET /`
* **Trigger:** Must respond with JSON *only* if the request header contains `Accept: application/nostr+json` (otherwise, upgrade to WebSocket or return a generic greeting).
* **Headers:** MUST include `Access-Control-Allow-Origin: *`.
* **Payload Structure:**
(Return the following as a JSON object)
  "name": "nwc-edge-relay"
  "description": "A stateless public NWC relay running on Cloudflare Workers."
  "software": "https://github.com/kohanucha/nwc-edge-relay"
  "supported_nips": [1, 11, 47]

### C. NIP-47: Nostr Wallet Connect (The Routing Engine)
*Note: The relay acts ONLY as an encrypted transport layer. It MUST NOT decrypt the `content` field.*
* **Allowed Kinds:** Prioritize routing for:
    * `23194` (Wallet Request)
    * `23195` (Wallet Response)
    * `23196`/`23197` (Wallet Notification)
* **Tag Enforcement (`#p` tag):** For kinds `23194`, `23195`, `23196`, and `23197`, the relay MUST ensure that a `p` tag (recipient pubkey) exists. 
* **Filter Matching Strategy:** * When an `EVENT` arrives, the relay iterates through active `REQ` filters in memory.
    * If the event's `kind` matches the filter's `kinds` array, AND the event's `p` tag matches the filter's `#p` array, forward the event to that connection via `["EVENT", <subscription_id>, <event_json>]`.

---

## 6. Implementation Plan (Phased)

### Phase 1: Infrastructure & Scaffolding
- [ ] Initialize `worker-rs` project named `nwc-edge-relay` and `git init`.
- [ ] Set remote: `git remote add origin git@github.com:kohanucha/nwc-edge-relay.git`.
- [ ] Configure `wrangler.toml` and Wasm-compatible `Cargo.toml`.

### Phase 2: Core Logic & Parsers (Pure Rust - No Worker Context)
- [ ] Create `nostr.rs` or `types.rs` for NIP-01 message structures.
- [ ] Implement signature validation using `secp256k1` via the `nostr` crate.
- [ ] Implement the **Filter Matching Engine** (checking if an event satisfies a filter).

### Phase 3: Cloudflare Worker Handlers
- [ ] Implement NIP-11 HTTP GET handler.
- [ ] Implement WebSocket upgrade logic (`worker::WebSocketPair`).
- [ ] Integrate the in-memory pub-sub mechanism. Ensure active connections are cleaned up upon disconnect.
- [ ] Add abuse prevention: drop incoming messages larger than 64KB.

### Phase 4: Unit Testing & Verification Logic
- [ ] **Unit Tests (`#[test]`):** * Test Event ID hashing and Signature validation.
    * Test Filter matching (e.g., ensure an event with `#p` tag matches a filter requiring that `#p` tag).
    * Test parsing of `EVENT`, `REQ`, and `CLOSE` messages from raw JSON strings.
- [ ] **Setup local verification script:** Create a simple test harness (or document `websocat` commands) to simulate:
    1.  Client A connecting and sending `REQ` for `kind: 23194`.
    2.  Client B connecting and sending a valid `EVENT` of `kind: 23194`.
    3.  Asserting Client A receives the broadcasted event.

### Phase 5: CI/CD Pipeline
- [ ] Create `.github/workflows/deploy.yml`.
- [ ] Include steps for: `cargo test` (unit tests), `cargo fmt`, and deployment via `cloudflare/wrangler-action@v3`.

---

## 7. Verification & Acceptance Criteria
Before considering the implementation complete, the AI Agent must ensure the following pass:
1.  **NIP-11 Test:** Running `curl -H "Accept: application/nostr+json" http://localhost:8787/` returns the correct JSON.
2.  **Stateless Subscription Test:** Sending a `REQ` immediately returns an `EOSE`.
3.  **Validation Test:** Sending a manipulated event (where signature does not match content) results in an `OK` false message and is NOT broadcasted.
4.  **Routing Test:** An NWC request (`23194`) is only broadcasted to the WebSocket connection that explicitly requested that `#p` tag in its `REQ` filter.
