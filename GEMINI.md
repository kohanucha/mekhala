# Project: Mekhala

A high-performance, 100% stateless Nostr relay built with **Rust** for **Cloudflare Workers**, specifically optimized for **NIP-47 (Nostr Wallet Connect)**.

## Project Overview
The relay acts as an ephemeral "routing engine" between Lightning wallet applications and nodes. Unlike traditional relays, it operates entirely in-memory with zero persistent storage, ensuring maximum privacy and minimal latency. It is strictly enforced for personal NWC use.

### Key Technologies
- **Rust**: Core logic compiled to WebAssembly for high-performance verification.
- **Cloudflare Workers**: Global edge runtime.
- **Durable Objects**: Manages stateful WebSocket connections and subscriptions using the **Hibernation API** for cost efficiency.
- **Nostr Protocol**: Implements NIP-01 (Basic), NIP-11 (Relay Information), and NIP-47 (Wallet Connect).

### Architecture
- **Stateless Routing**: Events are verified and broadcasted to active subscribers instantly. No event history is kept.
- **WebSocket Hibernation**: Subscription filters are serialized into WebSocket "attachments," allowing the Durable Object to hibernate when idle and resume state upon new messages.
- **High-Performance Verification**: ID verification is optimized using tuple-based serialization to avoid `serde_json::Value` overhead and unnecessary allocations.
- **Strict NWC Focus**: Only NWC-related event kinds (13194, 23194-23197) are processed. General Nostr kinds (0, 1) are rejected.
- **Connection Capping**: Limits concurrent connections (default 100) per Durable Object to prevent resource exhaustion, returning HTTP 429 when exceeded.
- **Optional Authentication**: Authorization via a secret path (e.g., `wss://relay.com/<SECRET>`) is supported but optional. If `RELAY_SECRET` is unset, the relay functions as a public relay on the root path.

## Project Structure
- `.github/workflows/`: CI/CD pipelines.
- `src/`: Core Rust source code.
    - `lib.rs`: Worker entry point and Durable Object implementation.
    - `relay.rs`: Core Nostr relay logic, filters, and security limits.
    - `nwc_client.rs`: NIP-47 client implementation for LN Address flows.
    - `lnurl.rs`: LUD-06/LUD-16 (LN Address) to NWC bridging logic.
    - `utils.rs`: Shared utility functions (CORS, Constant-time EQ, etc.).
- `test/`: Integration tests.
    - `setup-kv.js`: Utility to seed KV for testing LN Addresses.
    - `test-relay.js`: Comprehensive integration tests using `nostr-tools`.
- `build.sh`: Build script for compiling Rust to Wasm.
- `wrangler.toml`: Main configuration and security limits.

## Configuration (Environment Variables)
Mekhala is highly configurable via `wrangler.toml`:
- `MAX_CONNECTIONS`: Max concurrent WebSockets per Durable Object (Default: 100).
- `MAX_FILTER_ITEMS`: Max items allowed in filter arrays (ids, authors, etc.) (Default: 100).
- `MAX_EVENT_TAGS`: Max tags allowed per event (Default: 100).
- `MAX_CONTENT_LENGTH`: Max event content size in bytes (Default: 32768).
- `RELAY_SECRET`: Optional secret path for private relay mode.
- `MEKHALA_NWC_KV`: KV namespace binding for LN Address mapping.

## Building and Running

### Key Commands
- **Build**: `./build.sh`
- **Local Development**: `npx wrangler dev`
- **Unit Tests**: `cargo test` (Core logic, crypto, and filters).
- **Integration Tests**: `cd test && npm test` (Full protocol and E2E flows).

## Development Conventions

### Coding Standards
- **Strict NWC Enforcement**: Reject any event or subscription not matching NWC kinds (13194, 23194-23197).
- **Mandatory Filter Narrowing**: All `REQ` filters must include at least one criterion (`ids`, `authors`, `#p`, `#e`) to prevent broad snooping.
- **Security First**: Use constant-time comparisons for secrets and random 16-byte IVs for NIP-04 encryption.
- **Performance Focused**: Use `serde_json::to_string(&(0, ...))` for fast NIP-01 ID verification.

### Testing Practices
- **100% Coverage**: Core business logic in `relay.rs` and `nwc_client.rs` must have full unit test coverage.
- **Integration**: Every new feature or protocol restriction must be verified with `test-relay.js`.
- **Reproducibility**: Bug fixes must include a failing test case in `cargo test` or `test-relay.js` before the fix is applied.
