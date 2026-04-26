# Project: nwc-edge-relay

A high-performance, 100% stateless Nostr relay built with **Rust** for **Cloudflare Workers**, specifically optimized for **NIP-47 (Nostr Wallet Connect)**.

## Project Overview
The relay acts as an ephemeral "routing engine" between Lightning wallet applications and nodes. Unlike traditional relays, it operates entirely in-memory with zero persistent storage, ensuring maximum privacy and minimal latency.

### Key Technologies
- **Rust**: Core logic compiled to WebAssembly for high-performance verification.
- **Cloudflare Workers**: Global edge runtime.
- **Durable Objects**: Manages stateful WebSocket connections and subscriptions using the **Hibernation API** for cost efficiency.
- **Nostr Protocol**: Implements NIP-01 (Basic), NIP-11 (Relay Information), and NIP-47 (Wallet Connect).

### Architecture
- **Stateless Routing**: Events are verified and broadcasted to active subscribers instantly. No event history is kept.
- **WebSocket Hibernation**: Subscription filters are serialized into WebSocket "attachments," allowing the Durable Object to hibernate when idle and resume state upon new messages.
- **High-Performance Verification**: ID verification is optimized using byte-level comparison to avoid unnecessary string allocations.
- **Optional Authentication**: Authorization via a secret path (e.g., `wss://relay.com/<SECRET>`) is supported but optional. If `RELAY_SECRET` is unset, the relay functions as a public relay on the root path.
- **Graceful Error Handling**: WebSocket state serialization failures are reported to clients via NIP-01 `NOTICE` messages instead of dropping connections.

## Project Structure
- `.github/workflows/`: CI/CD pipelines.
    - `build-and-deploy.yml`: Production deployment to Cloudflare.
    - `pr.yml`: Automated tests and checks for Pull Requests.
- `src/`: Core Rust source code.
    - `lib.rs`: Worker entry point and Durable Object implementation.
    - `relay.rs`: Nostr relay logic, including event verification and filter matching.
    - `utils.rs`: Shared utility functions.
- `test/`: Integration tests.
    - `package.json`: Node.js dependencies for testing.
    - `test-relay.js`: Node.js tests using `nostr-tools`.
- `build.sh`: Build script for compiling Rust to Wasm and preparing the worker.
- `wrangler.toml`: Cloudflare Workers configuration (uses `new_sqlite_classes` for Free Plan compatibility).
- `Cargo.toml`: Rust dependencies and workspace configuration.
- `rust-toolchain.toml`: Rust version and target configuration.

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
- **Protocol Constants**: Use named constants (e.g., `KIND_NWC_REQUEST`) instead of magic numbers for Nostr kinds.
- **Performance Focused**: Prefer zero-allocation patterns and iterator-based tag matching to keep CPU/Memory usage within edge runtime limits.

### Testing Practices
- New features must include both Rust unit tests (in `src/relay.rs`) and integration tests (in `test/test-relay.js`).
- Always verify that `13194` (Info) events are routed statelessly and not stored.

## Future Improvements
- **WebSocket Tagging**: Cloudflare Durable Objects support WebSocket Tags, which allow for O(1) message broadcasting to specific subsets of connections. Currently, this cannot be used in a standard Nostr relay because tags can only be applied when *accepting* a connection, while Nostr clients provide their pubkey/filters *after* connecting (via NIP-01 `REQ` messages). If Cloudflare updates their API to allow modifying tags on an active, accepted WebSocket, this relay should be updated to use them for significantly improved routing performance.
