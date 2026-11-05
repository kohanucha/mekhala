# Mekhala - เมขลา ⚡️

**A private, stateless Nostr relay optimized for Nostr Wallet Connect (NIP-47) on Cloudflare Workers.**

> According to legend, the phenomena of lightning and thunder are produced by the flashing of Manimekhala's crystal ball.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Rust: Wasm32](https://img.shields.io/badge/Rust-Wasm32-orange.svg)](https://www.rust-lang.org/)
[![Cloudflare: Durable Objects](https://img.shields.io/badge/Cloudflare-Durable%20Objects-7A3E9D.svg)](https://developers.cloudflare.com/durable-objects/)

---

## 📖 Overview

Mekhala is a specialized, ephemeral Nostr relay built in Rust and compiled to WebAssembly. It acts as a private communication bridge between Lightning wallet applications (e.g. Alby, Amethyst) and your wallet node:

- **No event history:** Events are validated, routed, and dropped. The only persisted state is subscription metadata plus cached wallet-service info events, kept so hibernated connections can resume.
- **Standard NIP-01 routing:** Events reach subscriptions addressed by `authors`/`#p`, by `ids`, or by `#e`, with every index persisted so routing still works after hibernation. Cached `13194` info events are also replayed to `#p`-only subscriptions (the NWA discovery pattern).
- **WebSocket Hibernation:** Uses Cloudflare Durable Objects to hibernate idle connections, waking up seamlessly when new events arrive to save resources.
- **Strictly Specialized:** Only handles NWC-related event kinds (`5` for deleting cached wallet info, `13194`, `23194-23197`). Social and generic Nostr events are rejected.

---

## 📦 Quick Setup

1. **Fork & Deploy:**
   - Fork this repository to your GitHub account.
   - Connect it to your [Cloudflare Dashboard](https://dash.cloudflare.com/) via **Workers & Pages** -> **Create application** -> **Connect to Git** and deploy.
2. **Configure Private Path:**
   - Under your Worker's **Settings** -> **Variables and Secrets**, add a Secret named `RELAY_SECRET`.
   - Set it to a long, secure, unique path identifier.
3. **Connect Your Wallet:**
   - **Your Relay URL:** `wss://your-worker.workers.dev/<your-secret-path>`

> [!WARNING]
> **Lightning Address Bridge (Experimental):** The feature to bridge Lightning Addresses (e.g., `you@domain.com`) to NWC is highly experimental, under development, and not yet ready for production or general use.

---

## ⚙️ Configuration & Limits

Configurable via `wrangler.toml` or environment variables:

| Variable | Default | Description |
|---|---|---|
| `RELAY_SECRET` | *None* | Optional secret path parameter for private routing. |
| `WALLET_REGION` | `apac` | Physical DO location (`apac`, `weur`, `wnam`). |
| `MAX_CONNECTIONS` | `100` | Max concurrent WebSocket connections per DO. |
| `MAX_CONTENT_LENGTH` | `98304` | Max event `content` size in bytes. Covers the largest NIP-44 payload (87,472 base64 chars) plus the JSON envelope. |

---

## 🛠 For Developers

### Common Commands
- **Build WASM:** `./scripts/build.sh` (Requires Rust + `wasm32` target + `worker-build`)
- **Local Dev:** `npx wrangler dev` (Runs locally on port `8787`)
- **Unit Tests:** `cargo test`
- **Integration Tests:** `./scripts/test.sh` (Runs E2E compilation, DO setup, and Node.js checks)

### Coding Standards
Mekhala compiles with `panic = "abort"`. To prevent isolates from crashing:
- **No panics:** Never use `unwrap()` or `expect()`. Handle errors with `?` or `match`.
- **Safe indexing:** Use `.get()` for slice/array access instead of bracket notation.
- **Storage state:** Every subscription update must call `sync()` to ensure active filters survive DO hibernation.

---

## 📄 License

Mekhala is open-source software under the [MIT License](LICENSE).
