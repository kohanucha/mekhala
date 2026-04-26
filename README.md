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

## 🔒 Securing Your Relay (Optional but Recommended)
By default, the relay is public. To prevent unauthorized usage and ensure privacy, you can secure it with a secret path. 

**Note:** If you choose not to set a `RELAY_SECRET`, anyone can use your relay, which may lead to Cloudflare limits being hit or your Durable Object crashing if abused.

1. **Generate a Secret:**
   Run the following command in your terminal to generate a secure, random string:
   ```bash
   ./generate_secret.sh
   ```
2. **Add to Cloudflare:**
   - Go to your [Cloudflare Dashboard](https://dash.cloudflare.com/).
   - Go to **Workers & Pages** -> click on your `nwc-edge-relay`.
   - Go to **Settings** -> **Variables and Secrets**.
   - Under **Environment Variables**, click **Add variable**.
   - **Name:** `RELAY_SECRET`
   - **Value:** (Paste the secret from step 1)
   - Click **Deploy** to save.

4. **Set Wallet Region (Optional):**
   To minimize latency, you can specify a preferred geographic region for your relay:
   - Go to **Settings** -> **Variables and Secrets**.
   - Add a Variable named `WALLET_REGION`.
   - Set the Value to a Cloudflare location hint (e.g., `apac` for Asia, `weur` for Europe, `wnam` for US West).
   - Click **Deploy** to save.

3. **Your Relay URL:**
   - **Private mode:** `wss://your-relay.your-subdomain.workers.dev/<YOUR_SECRET>`
   - **Public mode:** `wss://your-relay.your-subdomain.workers.dev/` (leave `RELAY_SECRET` unset)

---

## 📦 Deploy Your Own (Automatic CI/CD)

Choose one of the following two ways to automatically deploy your relay.

### Option 1: Cloudflare Git Integration (Simpler)
This is the easiest setup. Cloudflare handles everything, but builds can take 4-5 minutes as Rust tools are re-installed each time.

1. **Fork or Clone** this repository to your GitHub account.
2. **Log in** to your [Cloudflare Dashboard](https://dash.cloudflare.com/).
3. Go to **Workers & Pages** -> **Create application** -> **Connect to Git**.
4. Select your repository and click **Save and Deploy**.

#### 🚀 Faster Builds (Recommended)
By default, Cloudflare runs everything in one step. To enable specialized caching and speed up your builds:
1. Go to your Worker in the Cloudflare Dashboard.
2. Go to **Settings** -> **Builds**.
3. Set the following fields:
   - **Build command**: `./build.sh`
   - **Build output directory**: `build/worker`
   - **Deployment command**: `npx wrangler deploy`

### Option 2: GitHub Actions (Faster Build)
This method uses aggressive caching to reduce build times to **~30 seconds**.

1. **Fork or Clone** this repository.
2. In your GitHub repo, go to **Settings** -> **Secrets and variables** -> **Actions** and add:
   - `CLOUDFLARE_API_TOKEN`: Your Cloudflare API Token (Edit Cloudflare Workers template).
   - `CLOUDFLARE_ACCOUNT_ID`: Your Cloudflare Account ID (found in the dashboard sidebar).
3. **Push to main** and the GitHub Action will handle the rest!

### Pull Request Checks
Every time you open a Pull Request, an automated workflow will:
- Run Rust Unit Tests.
- Build the Worker.
- Run Integration Tests against a local Miniflare instance.

This ensures that your `main` branch remains stable. You can eventually enable **Branch Protection** in GitHub Settings to require these checks to pass before merging.

---

## 💻 Development & Testing
- **Local Dev:** `./build.sh && wrangler dev`
- **Unit Tests:** `cargo test`
- **Integration Tests:** `cd test && npm i && node test-relay.js`

---

## ⚖️ License
[MIT License](LICENSE)
