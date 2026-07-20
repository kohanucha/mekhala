# Mekhala Agent Context

## Overview
Cloudflare Worker (TypeScript) implementing Nostr Wallet Connect (NIP-47) relay with WebSocket Hibernation.

## Key Commands
| Action | Command | Notes |
|--------|---------|-------|
| Type-check | `./scripts/build.sh` or `npx tsc --noEmit` | |
| Dev server | `npx wrangler dev` | Local on port 8787 |
| Integration tests | `./scripts/test.sh` | Full pipeline: typecheck → wrangler → node |

## Build Requirements
- Node.js + npm

## Architecture
- **Durable Object**: `CloudflareTransport` (src/cloudflare/transport.ts)
- **Modules**: auth.ts, engine.ts, server.ts, cloudflare/, lnaddress/, nostr/, util/

## Critical Dependencies
- `@noble/curves` for Schnorr signatures
- `@noble/hashes` for SHA-256, HKDF, HMAC
- `@noble/ciphers` for ChaCha20

## Required Env Vars
- `RELAY_SECRET` - password (set via Cloudflare dashboard)
- `WALLET_REGION` - optional: apac/weur/wnam (default: apac)
- `MAX_CONNECTIONS` - optional, default: 100
- `MAX_SUBSCRIPTIONS_PER_CONNECTION` - optional, default: 100
- `MAX_FILTER_ITEMS` - default: 10
- `MAX_EVENT_TAGS` - default: 10
- `MAX_CONTENT_LENGTH` - default: 16384 (16 KB)

## Wrangler Config
- Entry: `src/cloudflare/index.ts`
- Durable Object: `CloudflareTransport`
- Compatibility date: 2026-04-25

## Common Gotchas
1. **DO state**: Use `ctx.storage` (DurableObjectState.storage)
2. **WebSocket messages**: WebSocket hibernation is implicit in the DO lifecycle — no manual tag management needed in TS
3. **`sync()` required for persistence**: Every `subscribe()`/`unsubscribe()` must call `sync()`. The in-memory index is lost on DO hibernation — only `state.storage()` survives. Removing `sync()` breaks event routing after wake.

## Agent skills

### Issue tracker

GitHub Issues via GitHub MCP. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout. See `docs/agents/domain.md`.

## Testing Conventions

### Test file layout
- Unit tests live in `src/` as `*.test.ts` files alongside their source module.
- Integration tests live in `test/` as `*.js` files.
- Use `describe`/`it` from `vitest` for unit tests.

### Shared test utilities
- Cross-module test helpers like `MockStorage` live in `src/common/test_helpers.ts`.

### Time mocking
- Use `vi.setSystemTime()` (vitest) for time mocking in unit tests.

## Coverage Gate
- Maintain ≥90% line coverage on testable modules (everything outside `cloudflare/`)
- Run: `npx vitest --coverage --coverage.exclude='src/cloudflare/**'`
