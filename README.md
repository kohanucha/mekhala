# Mekhala - เมขลา ⚡️

**A private, stateless Nostr relay optimized for NIP-47 (NWC) on Cloudflare Workers.**

> According to legend, the phenomena of lightning and thunder is produced from the flashing of Manimekhala's crystal ball.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

---

## 📖 Overview

Mekhala is a specialized relay designed to act as a private communication bridge between Lightning applications (like Amethyst or Alby) and your wallet:

- **Stateless Architecture:** No persistent database is used. Events are processed in-memory and routed to active subscribers instantly.
- **Privacy-First:** Access is restricted via a secret path to ensure the relay remains for personal use only.
- **Resource Efficient:** Built with Rust/WebAssembly to run within the constraints of the Cloudflare Workers Free Tier.

---

## 📦 Setup Guide

1. **Fork this repository:** Create a copy of this project in your own GitHub account.
2. **Deploy to Cloudflare:** 
   - Log in to your [Cloudflare Dashboard](https://dash.cloudflare.com/).
   - Navigate to **Workers & Pages** -> **Create application** -> **Connect to Git**.
   - Select your `mekhala` repository and click **Save and Deploy**.
3. **Set up your Secret Path:** 
   - In your Worker dashboard, go to **Settings** -> **Variables and Secrets**.
   - Add a new **Secret** named `RELAY_SECRET`. 
   - Enter a long, unique string (e.g., `my-private-relay-secret-123`). This string will be used as your private access path.
   - Click **Save and Deploy**.

**Your Relay URL:** `wss://your-worker-name.workers.dev/your-secret-path`

---

## ⚠️ Project Status

### 🚧 Lightning Address Bridge (Experimental)
The feature to bridge Lightning Addresses (e.g., `you@domain.com`) to NWC is **currently in progress**. It requires advanced manual configuration of Cloudflare KV namespaces and is not yet recommended for general use.

### External Wallet Services
If you use third-party services like Alby, they may require connections to their own relays. It is recommended to keep your wallet connected to **both** this private relay and your provider's default relay to ensure full compatibility.

---

## 🔒 Security & Limits

Mekhala is pre-configured with restrictive limits optimized for a single user:
- **Max Connections:** 20 (Supports multiple devices for one user).
- **Max Content Length:** 16 KB (Fits encrypted invoices and history).
- **Max Tags:** 10 (Restricts metadata bloating).

---

## 🛠 For Developers

### Supported Protocols
- **NIP-01:** Basic protocol (Stateless REQ/EVENT flow).
- **NIP-11:** Relay Information (NWC capability discovery).
- **NIP-47:** Nostr Wallet Connect (Info, Request, Response, and Notifications).
- **LUD-06/16:** (In Progress) LN Address to NWC bridging via Cloudflare KV.

### Local Development
- **Build:** `./scripts/build.sh` (Requires `worker-build` and `wasm-pack`).
- **Dev Server:** `npx wrangler dev`.
- **Unit Tests:** `cargo test`.
- **Integration Tests:** `./scripts/test.sh` (Comprehensive Rust + Node.js E2E suite).

### Environment Variables
- `RELAY_SECRET`: Path-based secret for authentication.
- `WALLET_REGION`: (Optional) Physical location for the Durable Object (`apac`, `weur`, `wnam`).
- `MAX_CONNECTIONS`, `MAX_FILTER_ITEMS`, `MAX_EVENT_TAGS`, `MAX_CONTENT_LENGTH`: Configurable limits in `wrangler.toml`.
