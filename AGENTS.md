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
- **Modules**: auth.rs, lib.rs, server.rs, cloudflare/, lnaddress/, model/, nostr/, util/

## Critical Dependencies
- `worker` 0.8.x runtime
- `k256` for Schnorr signatures

## Required Env Vars
- `RELAY_SECRET` - password (set via Cloudflare dashboard)
- `WALLET_REGION` - optional: apac/weur/wnam (default: apac)
- `MAX_CONNECTIONS` - optional, default: 100
- `MAX_SUBSCRIPTIONS_PER_CONNECTION` - optional, default: 100
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
4. **`sync()` required for persistence**: Every `subscribe()`/`unsubscribe()` must call `sync()`. The in-memory index is lost on DO hibernation — only `state.storage()` survives. Removing `sync()` breaks event routing after wake. Regression test: `test_hibernation_contract`.

## Agent skills

### Issue tracker

GitHub Issues via GitHub MCP. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout. See `docs/agents/domain.md`.

## Coverage Gate
- Maintain ≥90% line coverage on testable modules (everything outside `cloudflare/`)
- Run: `cargo llvm-cov --ignore-filename-regex 'cloudflare/' --fail-under-lines 90`

> All worker-dependent modules live under `cloudflare/`. The core relay logic (`nostr/*`, `auth.rs`, `lnaddress/*`) has zero `worker` crate dependency and is fully testable.
