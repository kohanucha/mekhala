# Specification: Ultra-Lite Private NWC Relay (Rust)

## 1. Project Overview
A specialized, high-performance, and minimal Nostr relay built in **Rust**. Its sole purpose is to support **NIP-47 (Nostr Wallet Connect)** for private home use. The relay acts as a whitelisted bridge between an Alby Hub (Wallet Service) and an Alby Go (Client App).

## 2. Technical Stack
* **Language:** Rust (Latest Stable)
* **Async Runtime:** `tokio`
* **WebSocket:** `tokio-tungstenite`
* **Nostr Protocol:** `nostr-sdk` (specifically using the `nostr` core crate)
* **Database:** `sled` (Embedded KV) for persistent Whitelist storage.
* **CLI Parser:** `clap`
* **Containerization:** Docker (Multi-stage build)
* **Version Control:** Git

## 3. Pre-Implementation Instructions for AI Agent
**CRITICAL:** Before generating any code, you must fetch and analyze the following NIPs from the [nostr-protocol/nips](https://github.com/nostr-protocol/nips) repository:

1.  **[NIP-47: Nostr Wallet Connect](https://github.com/nostr-protocol/nips/blob/master/47.md)**
2.  **[NIP-01: Basic protocol](https://github.com/nostr-protocol/nips/blob/master/01.md)**
3.  **[NIP-11: Relay Information Document](https://github.com/nostr-protocol/nips/blob/master/11.md)**

## 4. Coding Philosophy (Crucial)
* **Simple & Idiomatic:** Write Rust code that is easy to read. Avoid overly complex macro-programming or deep abstraction layers. 
* **Beginner-Friendly:** Use clear variable names and provide comments for complex logic so a beginner can follow along.
* **Efficient:** Leverage Rust's zero-cost abstractions and `tokio`'s concurrency without sacrificing code clarity.
* **Single Responsibility:** Keep functions small and focused.

## 5. Core Requirements & Features

### A. Git Initialization
* Initialize a Git repository at the start.
* Provide a standard `.gitignore` for Rust (targeting `/target`, `Cargo.lock` if appropriate, and DB files like `*.sled`).

### B. Minimal NIP Support
* **NIP-01:** Basic WebSocket messaging and event handling.
* **NIP-11:** Serve a static JSON document via HTTP for relay metadata.
* **NIP-47:** Efficient bridging of Kinds 23194 (Request) and 23195 (Response).

### C. Strict Whitelist Security
* **Zero-Trust Policy:** Only allow `pubkey`s explicitly registered via CLI.
* **Verification Rules:**
    1.  **Incoming EVENTs:** Signature verification is mandatory. `event.pubkey` MUST be whitelisted.
    2.  **Incoming REQs:** The `authors` or `#p` tags MUST only contain whitelisted pubkeys. **Disconnect immediately** if a non-whitelisted pubkey is requested.
    3.  **Bridge Mode:** Acts as a volatile, in-memory message router. No persistent event storage.

### D. CLI Management
* `--add <PUBKEY_HEX>`: Register a hex-encoded pubkey.
* `--remove <PUBKEY_HEX>`: Revoke a pubkey's access.
* `--list`: Display all whitelisted pubkeys.
* `--run`: Start the relay server.

### E. Docker Integration
* **Multi-stage Build:** Build with `rust:alpine`, run with `alpine`.
* **Volume Mapping:** Persistent storage for `/data` (where `sled` resides).

## 6. Implementation Plan

### Phase 1: Setup & CLI
- [ ] `git init` and setup `.gitignore`.
- [ ] Build CLI commands using `clap`.
- [ ] Implement `sled` storage for the whitelist.

### Phase 2: Relay Engine & Security Guard
- [ ] Setup `tokio-tungstenite` server.
- [ ] Implement a minimal NIP-11 HTTP handler.
- [ ] **Security Interceptor:** Build the validation engine (Signature verify + Whitelist check).

### Phase 3: Dockerization
- [ ] Create `Dockerfile` (Multi-stage) and `docker-compose.yml`.

## 7. Final Note for Claude
> When writing the Rust code, prioritize **clarity over cleverness**. Ensure the NWC flow is robust. If an unauthorized user connects, drop them silently. Keep the codebase "Lite" and the binary size small.
