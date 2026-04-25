# nwc-edge-relay ⚡️

**High-performance, 100% stateless Nostr relay for NWC, built with Rust for Cloudflare Workers.**

[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)](#)
[![Cloudflare Workers](https://img.shields.io/badge/Cloudflare-Workers-F38020?logo=cloudflare&logoColor=white)](#)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

---

## 🎯 The Concept
**nwc-edge-relay** is a specialized "routing engine" for **NIP-47 (Nostr Wallet Connect)**. It replaces heavy, database-backed relays with a lightweight, in-memory bridge between your wallet apps and your Lightning node.

### Why use it?
- **Zero Latency:** No database I/O. Events are routed instantly in memory.
- **Zero Maintenance:** No database to scale, back up, or manage.
- **Privacy First:** Ephemeral routing. Your NWC traffic is never logged or stored.
- **100% Stateless:** No Durable Object storage is used. All data exists only in-flight.
- **Cost Efficient:** Uses **WebSocket Hibernation** to minimize resource usage.

---

## 🚀 Key Features
- **Global Edge:** Runs on Cloudflare's network, physically close to you.
- **Secure:** Instant Rust-powered signature verification.
- **NWC Focused:** Supports NIP-47 routing for Info (13194), Requests (23194), Responses (23195), and Notifications (23196/23197).
- **Auto-Build:** Fully automated environment setup and Wasm compilation.

---

## ⚙️ Configuration
You can customize your relay's identity (NIP-11 metadata) by editing the `[vars]` section in `wrangler.toml`:
- `RELAY_NAME`: The name of your relay.
- `RELAY_DESCRIPTION`: A short description of the service.
- `RELAY_PUBKEY`: The administrator's hex pubkey.
- `RELAY_CONTACT`: Contact information (URI).
- `RELAY_SOFTWARE`: Link to the software repository.
- `RELAY_VERSION`: Software version.

---

## 🌍 Quick Setup (Alby Hub / Alby Go)
1. **URL:** `wss://your-relay-name.workers.dev`
2. **Setup:** Go to your wallet's **App Connection settings** -> **Advanced**.
3. **Connect:** Set the **Relay URL** to your edge relay. Your wallet and node are now connected via the edge!

---

## 📦 Deploy Your Own (Automatic CI/CD)

The recommended way to deploy is using Cloudflare's built-in **Git Integration**. This automatically builds and deploys your relay every time you push to GitHub.

1. **Fork or Clone** this repository to your GitHub account.
2. **Log in** to your [Cloudflare Dashboard](https://dash.cloudflare.com/).
3. Go to **Workers & Pages** -> **Create application** -> **Connect to Git**.
4. Select your repository and click **Save and Deploy**.

Cloudflare will automatically detect the build settings from `wrangler.toml` and handle the Rust compilation for you!

---

## 💻 Development & Testing
- **Local Dev:** `./build.sh && wrangler dev`
- **Unit Tests:** `cargo test`
- **Integration Tests:** `cd test && npm i && node test-relay.js`

---

## ⚖️ License
[MIT License](LICENSE)
