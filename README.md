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

## 📦 Deploy Your Own (Automatic CI/CD)

Choose one of the following two ways to automatically deploy your relay.

### Option 1: Cloudflare Git Integration (Simpler)
This is the easiest setup. Cloudflare handles everything, but builds can take 4-5 minutes as Rust tools are re-installed each time.

1. **Fork or Clone** this repository to your GitHub account.
2. **Log in** to your [Cloudflare Dashboard](https://dash.cloudflare.com/).
3. Go to **Workers & Pages** -> **Create application** -> **Connect to Git**.
4. Select your repository and click **Save and Deploy**.

### Option 2: GitHub Actions (Faster Build & Rich Metadata)
This method uses aggressive caching to reduce build times to **~30 seconds** and correctly displays Git metadata (branch, commit) in the Cloudflare dashboard.

1. **Fork or Clone** this repository.
2. **Create a GitHub App** for your repository:
   - Go to **Settings** -> **Developer settings** -> **GitHub Apps** -> **New GitHub App**.
   - **Homepage URL**: Your repository URL (e.g., `https://github.com/your-username/nwc-edge-relay`).
   - **Callback URL**: Leave blank.
   - **Webhook**: Uncheck "Active" (not needed).
   - **Permissions**: **Repository permissions** -> **Contents** (Read-only) & **Metadata** (Read-only).
   - Click **Create GitHub App**.
3. **Configure the App**:
   - Note the **App ID**. (Note: Use the App ID even if GitHub suggests using Client ID).
   - Scroll down to the **Private keys** section and click **Generate a private key**. This will download a `.pem` file.
   - Install the app: Click **Install App** in the sidebar and install it to your repository.
4. **Add GitHub Secrets**:
   - In your GitHub repo, go to **Settings** -> **Secrets and variables** -> **Actions**.
   - Add the following secrets:
     - `CLOUDFLARE_API_TOKEN`: Your Cloudflare API Token.
     - `CLOUDFLARE_ACCOUNT_ID`: Your Account ID.
     - `APP_ID`: Your GitHub App ID.
     - `APP_PRIVATE_KEY`: The entire contents of the downloaded `.pem` file.
5. **Push to main** and the GitHub Action will handle the rest!

---

## 💻 Development & Testing
- **Local Dev:** `./build.sh && wrangler dev`
- **Unit Tests:** `cargo test`
- **Integration Tests:** `cd test && npm i && node test-relay.js`

---

## ⚖️ License
[MIT License](LICENSE)
