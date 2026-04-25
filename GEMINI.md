# Project: nwc-edge-relay

A high-performance, 100% stateless Nostr relay built with **Rust** for **Cloudflare Workers**, specifically optimized for **NIP-47 (Nostr Wallet Connect)**.

## Project Overview
The relay acts as an ephemeral "routing engine" between Lightning wallet applications and nodes. Unlike traditional relays, it operates entirely in-memory with zero persistent storage, ensuring maximum privacy and minimal latency.

### Key Technologies
- **Rust**: Core logic compiled to WebAssembly.
- **Cloudflare Workers**: Global edge runtime.
- **Durable Objects**: Manages stateful WebSocket connections and subscriptions using the **Hibernation API** for cost efficiency.
- **Nostr Protocol**: Implements NIP-01 (Basic), NIP-11 (Relay Information), and NIP-47 (Wallet Connect).

### Architecture
- **Stateless Routing**: Events are verified and broadcasted to active subscribers instantly. No event history is kept.
- **WebSocket Hibernation**: Subscription filters are serialized into WebSocket "attachments," allowing the Durable Object to hibernate when idle and resume state upon new messages.

## Building and Running

### Prerequisites
- Rust toolchain (`wasm32-unknown-unknown` target).
- Node.js & npm.
- Cloudflare Wrangler CLI.

### Key Commands
- **Build**: `./build.sh`
  - Automatically ensures the Rust environment is set up and compiles the project using `worker-build`.
- **Local Development**: `npx wrangler dev`
  - Runs the relay in a local emulator (Miniflare).
- **Unit Tests**: `cargo test`
  - Runs Rust-based tests for event verification and filter matching.
- **Integration Tests**: `cd test && npm test`
  - Runs JavaScript tests using `nostr-tools` against a running relay instance.

## Development Conventions

### Coding Standards
- **Statelessness First**: Avoid adding persistent storage dependencies. All event routing should remain in-flight.
- **Strict Protocol Enforcement**: Incoming events must be instantly validated for ID integrity and Schnorr signatures.
- **NIP-47 Constraints**: Enforce the presence of `#p` tags for NWC request/response kinds.

### Configuration
- Relay metadata (NIP-11) is configurable via `[vars]` in `wrangler.toml`.
- Default values for `RELAY_NAME`, `RELAY_DESCRIPTION`, etc., should be kept generic; user-specific info should be provided via environment variables during deployment.

### Testing Practices
- New features must include both Rust unit tests (in `src/relay.rs`) and integration tests (in `test/test-relay.js`).
- Always verify that `13194` (Info) events are routed statelessly and not stored.
