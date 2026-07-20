# Mekhala - เมขลา ⚡️

**A private, stateless Nostr relay optimized for Nostr Wallet Connect (NIP-47) on Cloudflare Workers.**

> According to legend, the phenomena of lightning and thunder are produced by the flashing of Manimekhala's crystal ball.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.8-blue)](https://www.typescriptlang.org/)
[![Cloudflare: Durable Objects](https://img.shields.io/badge/Cloudflare-Durable%20Objects-7A3E9D.svg)](https://developers.cloudflare.com/durable-objects/)

---

## 📖 Overview

Mekhala is a specialized, ephemeral Nostr relay built in TypeScript. It acts as a private, zero-persistence communication bridge between Lightning wallet applications (e.g. Alby, Amethyst) and your wallet node:

- **100% Stateless:** No event history or database storage. All events are routed instantly in-memory.
- **WebSocket Hibernation:** Uses Cloudflare Durable Objects to hibernate idle connections, waking up seamlessly when new events arrive to save resources.
- **Strictly Specialized:** Only routes NWC-related event kinds (`13194`, `23194-23197`). Social and generic Nostr events are rejected.

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
| `MAX_CONTENT_LENGTH` | `65536` | Max Nostr event size in bytes (16KB to 64KB recommended). |

---

## 🛠 For Developers

### Common Commands
- **Type-check:** `npx tsc --noEmit`
- **Local Dev:** `npx wrangler dev` (Runs locally on port `8787`)
- **Integration Tests:** `./scripts/test.sh` (Runs type-check, starts DO, runs Node.js checks)

### Coding Standards
- **No panics:** Always use `?` or proper error handling.
- **Safe indexing:** Use `.get()` for array/slice access.
- **Storage state:** Every subscription update must call `sync()` to ensure active filters survive DO hibernation.

---

## 📄 License

Mekhala is open-source software under the [MIT License](LICENSE).
