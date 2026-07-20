# ADR 0001: Async Wallet Registry Persistence

## Status
Proposed

## Context
Mekhala's `NostrEngine` and `WalletRegistry` were originally designed as synchronous, pure modules. However, the requirement to persist subscription state to Cloudflare's Durable Object storage (which is asynchronous) led to a circular dependency: the engine had to return `StoreState` commands to the transport layer, which then performed the async I/O and sometimes called back into the engine or registry. This "shallow" design fragmented the state management logic.

## Decision
We will transition the core domain logic (`WalletRegistry` and `NostrEngine`) from synchronous to asynchronous. We will introduce a `Storage` trait that allows `WalletRegistry` to handle its own persistence directly.

## Consequences
- `NostrEngine` methods like `on_message` and `handle_verified_event` will become `async`.
- `WalletRegistry` methods will become `async`.
- `EngineResponse::StoreState` will be removed, making `NostrEngine` a pure routing engine.
- Increased complexity in unit tests (requiring async test support).
- Cleaner separation of concerns: Transport only handles I/O, Engine handles routing, and Registry handles state/indexing.
