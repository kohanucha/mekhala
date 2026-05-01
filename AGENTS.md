# Mekhala Agent Context

## Overview
Cloudflare Worker (Rust/WASM) implementing Nostr Wallet Connect (NIP-47) relay with WebSocket Hibernation.

## Key Commands
| Action | Command | Notes |
|--------|---------|-------|
| Build WASM | `./scripts/build.sh` | Requires Rust + wasm32 target + worker-build |
| Dev server | `npx wrangler dev` | Local on port 8787 |
| Unit tests | `cargo test` | Rust-only tests |
| Integration tests | `./scripts/test.sh` | Full pipeline: test → build → wrangler → node |

## Build Requirements
- `rustup target add wasm32-unknown-unknown`
- `cargo install worker-build`
- Node.js + npm

## Architecture
- **Durable Object**: `NwcRelay` (lib.rs:75)
- **Modules**: relay.rs, nwc_client.rs, nwc_relay.rs, lnurl.rs, utils.rs

## Critical Dependencies
- `worker` 0.8.x runtime
- `k256` for Schnorr signatures

## Required Env Vars
- `RELAY_SECRET` - password (set via Cloudflare dashboard)
- `WALLET_REGION` - optional: apac/weur/wnam (default: apac)
- `MAX_CONNECTIONS` - default: 20 (Optimized for personal NWC)
- `MAX_FILTER_ITEMS` - default: 10
- `MAX_EVENT_TAGS` - default: 10
- `MAX_CONTENT_LENGTH` - default: 16384 (16 KB)

## Wrangler Config
- Output: `build/worker/shim.mjs`
- Durable Object: `NwcRelay`
- Compatibility date: 2026-04-25

## Common Gotchas
1. **DO state**: Use `self.state.storage()`
2. **WebSocket tags**: Use `utils::HibernationState` trait
3. **Panic = Abort**: The project uses `panic = "abort"`. **NEVER** use `unwrap()` or `expect()`. Use `?`, `.get()` for indexing, and checked math to prevent isolate crashes.

## Agent skills

### Issue tracker

GitHub Issues. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout. See `docs/agents/domain.md`.
