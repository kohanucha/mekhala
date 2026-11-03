# Mekhala Agent Context

## Overview
Cloudflare Worker (Rust/WASM) implementing Nostr Wallet Connect (NIP-47) relay with WebSocket Hibernation.

## Key Commands
| Action | Command | Notes |
|--------|---------|-------|
| Build WASM | `./scripts/build.sh` | Requires Rust + wasm32 target + worker-build |
| Dev server | `npx wrangler dev` | Local on port 8787 |
| Unit tests | `cargo test` | Rust-only tests |
| Format | `cargo fmt --all -- --check` | rustfmt defaults, no `rustfmt.toml`; CI fails on drift |
| Integration tests | `./scripts/test.sh` | Full pipeline: test → build → wrangler → node |

## Build Requirements
- `rustup target add wasm32-unknown-unknown`
- `cargo install worker-build`
- Node.js + npm

## Architecture
- **Durable Object**: `CloudflareTransport` (`src/cloudflare/transport.rs`; wrangler `class_name = "CloudflareTransport"`)
- **Modules**: auth.rs, lib.rs, cloudflare/, lnaddress/, nostr/, util/

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
- Durable Object binding: `MEKHALA_NWC_DO` → class `CloudflareTransport`
- Compatibility date: 2026-04-30

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

## Testing Conventions

### Test file layout
- Each source file has a sibling `*_test.rs` wired via `#[cfg(test)] #[path = "..."] mod xxx_test;` — **no `mod tests { }` blocks in source files**.
- The only `#[cfg(test)]` item in a source file is the `mod xxx_test;` declaration. All test code (functions, helpers, impls) lives in the test file.

### Shared test utilities
- `MockStorage`, `simulate_hibernation`, and similar cross-module test helpers live in `src/common/test_helpers.rs`.
- Import with `use crate::common::test_helpers::*;`.

### Async tests
- Use `futures::executor::block_on(async { ... })` inside `#[test]` functions.

### Private field access
- Test files are child modules of the source file they test, so they can access private fields directly.
- Prefer **standalone functions** over inherent methods for test helpers (e.g. `fn new_engine()` instead of `impl NostrEngine<MockStorage> { pub fn new() }`).

### Time mocking
- `TEST_TIME` thread_local, `test_now()`, and `set_test_time()` live in `engine_test.rs`.
- Available to all tests in that file via direct module scope.

## Coverage Gate
- Measure **production code only**: test files (`*_test.rs`) and `cloudflare/`
  are excluded. Test files would otherwise count their own ~99% coverage into
  the denominator; `cloudflare/` needs a live workerd and is covered by the
  integration suite instead.
- Thresholds: **95% lines / 90% regions / 80% functions**
- Run:
  ```
  cargo llvm-cov --locked --ignore-filename-regex '(cloudflare/|_test\.rs$)' \
    --fail-under-lines 95 --fail-under-regions 90 --fail-under-functions 80
  ```

> All worker-dependent modules live under `cloudflare/`. The core relay logic (`nostr/*`, `auth.rs`, `lnaddress/*`) has zero `worker` crate dependency and is fully testable.

## Mutation Testing
- Coverage says the code was *executed*; mutation testing proves the tests
  *fail* when behaviour changes. Run it on demand before a release, not in CI
  (a full run takes ~15 minutes):
  ```
  cargo install cargo-mutants --locked   # once
  cargo mutants --in-place -t 60
  ```
  `--in-place` keeps the runtime sane (without it every mutant copies the tree
  and rebuilds all dependencies) and cannot be combined with `-j`/`--jobs`.
- Scope and documented equivalent mutants live in `.cargo/mutants.toml`.
  `exclude_re` patterns are matched against `file:line:col: description`, so
  prefer matching the description over line numbers, which shift.
- The run must end with `0 missed` and no timeouts. Survivors are listed in
  `mutants.out/missed.txt`; fix the test (assert the exact error/edge) rather
  than widening the exclusion.
- If a run is interrupted, `git status` may show a mutated source file; restore
  it with `git checkout -- <file>` before doing anything else.
