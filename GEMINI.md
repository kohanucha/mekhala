# Project: Mekhala

A high-performance, 100% stateless Nostr relay built with **TypeScript** for **Cloudflare Workers**, specifically optimized for **NIP-47 (Nostr Wallet Connect)**.

## Project Overview
The relay acts as an ephemeral "routing engine" between Lightning wallet applications and nodes. Unlike traditional relays, it operates entirely in-memory with zero persistent storage, ensuring maximum privacy and minimal latency. It is strictly enforced for personal NWC use.

### Key Technologies
- **TypeScript**: Core logic running on Cloudflare Workers runtime.
- **Cloudflare Workers**: Global edge runtime.
- **Durable Objects**: Manages stateful WebSocket connections and subscriptions using the **Hibernation API** for cost efficiency.
- **Nostr Protocol**: Implements NIP-01 (Basic), NIP-11 (Relay Information), NIP-44 (Encryption), and NIP-47 (Wallet Connect).

### Architecture
- **Stateless Routing**: Events are verified and broadcasted to active subscribers instantly. No event history is kept.
- **WebSocket Hibernation**: Subscription filters are persisted to Durable Object storage, allowing the DO to hibernate when idle and resume state upon new messages.
- **Strict NWC Focus**: Only NWC-related event kinds (13194, 23194-23197) are processed. General Nostr kinds (0, 1) are rejected.
- **Connection Capping**: Limits concurrent connections (default 100) per Durable Object to prevent resource exhaustion, returning HTTP 429 when exceeded.
- **Optional Authentication**: Authorization via a secret path (e.g., `wss://relay.com/<SECRET>`) is supported but optional. If `RELAY_SECRET` is unset, the relay functions as a public relay on the root path.

## Project Structure
- `.github/workflows/`: CI/CD pipelines.
- `src/`: Core TypeScript source code.
    - `cloudflare/`: Cloudflare Workers transport layer.
        - `index.ts`: Worker entry point.
        - `router.ts`: Request router and HTTP handlers.
        - `transport.ts`: Cloudflare Transport Durable Object (WebSocket, Hibernation).
        - `auth.ts`: Relay access control.
        - `kv.ts`: KV namespace operations.
        - `ws-handler.ts`: WebSocket message processing.
        - `http.ts`: HTTP response helpers.
        - `connection.ts`: Connection registry.
        - `config.ts`: Environment configuration.
        - `storage.ts`: Durable Object storage adapter.
    - `lnaddress/`: LN Address to NWC bridging logic.
    - `nostr/`: Nostr protocol implementation (NIPs, Event, Filter, Limits, Engine).
    - `common/`: Shared utilities, types, and test helpers.
        - `util.ts`: Hex/base64 encoding, logging helpers.
        - `nwc-error.ts`: NWC error types.
        - `nwc-transport.ts`: NWC transport and user store interfaces.
        - `test-helpers.ts`: Cross-module test utilities (MockStorage, MockTransport).
- `test/`: Integration tests (Node.js via nostr-tools).
- `scripts/`: Shell scripts for development and CI.
    - `build.sh`: Type-check script (`npx tsc --noEmit`).
    - `setup-kv.sh`: Script to initialize KV for local testing.
    - `test.sh`: Main entry point for running the full test suite.
- `wrangler.toml`: Main configuration and security limits.

## Configuration (Environment Variables)
Mekhala is highly configurable via `wrangler.toml`:
- `MAX_CONNECTIONS`: Max concurrent WebSockets per Durable Object (Default: 100).
- `MAX_FILTER_ITEMS`: Max items allowed in filter arrays (ids, authors, etc.) (Default: 10 - NWC filters are narrow).
- `MAX_EVENT_TAGS`: Max tags allowed per event (Default: 10 - NWC events are functional).
- `MAX_CONTENT_LENGTH`: Max event content size in bytes (Default: 16384 - Fits invoices and history).
- `RELAY_SECRET`: Optional secret path for private relay mode.
- `MEKHALA_NWC_KV`: KV namespace binding for LN Address mapping.

## Building and Running

### Key Commands
- **Test Everything**: `./scripts/test.sh`
  - **MANDATORY**: Run this script after every code change. It runs type-check, starts a local relay, and runs the Node.js integration suite.
- **Type-check**: `npx tsc --noEmit`
- **Local Development**: `npx wrangler dev`
- **Unit Tests**: `npx vitest run`
- **Integration Tests**: `cd test && npm test`

## Development Conventions

### Coding Standards
- **Test Before Push**: NEVER commit or push code without a successful `./scripts/test.sh` run.
- **Strict NWC Enforcement**: Reject any event or subscription not matching NWC kinds (13194, 23194-23197).
- **Security-Hardened Limits**: Use restrictive limits optimized for personal NWC use.
- **Mandatory Filter Narrowing**: All `REQ` filters must include at least one criterion (`ids`, `authors`, `#p`, `#e`) to prevent broad snooping.
- **Security First**: Use constant-time comparisons for secrets and proper encryption (NIP-04, NIP-44).
- **`sync()` for Persistence**: Always call `sync()` after `subscribe()`/`unsubscribe()` to persist in-memory state to Durable Object storage.

### Testing Practices
- **Coverage**: Core business logic in `nostr/` must have unit test coverage.
- **Integration**: Every new feature or protocol restriction must be verified with `test/run-all.js`.
- **Reproducibility**: Bug fixes must include a failing test case before the fix is applied.
