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
- **Standard NIP-01 routing:** Events reach subscriptions addressed by `authors`/`#p`, by `ids`, or by `#e`, with every index persisted so routing still works after hibernation. Cached `13194` info events are replayed to filters that name the wallet's own pubkey (`authors`, or a `p` tag carrying that pubkey); they are not discoverable through an app's pubkey.
- **Plain NWC only:** Connections are the standard `nostr+walletconnect://` kind, where the client already knows the wallet pubkey (Alby Go ↔ Alby Hub, or any app that accepts an NWC connection string). The wallet-auth / one-click Nostr flow, where a wallet announces itself to an app through a `p` tag on its info event, is not served: a `{kinds:[13194], "#p":[<app>]}` `REQ` replays no events. (A live publish still routes like any NIP-01 event; only the replay path is missing.)
- **WebSocket Hibernation:** Uses Cloudflare Durable Objects to hibernate idle connections, waking up seamlessly when new events arrive to save resources.
- **Strictly Specialized:** Only handles NWC-related event kinds (`5` for deleting cached wallet info, `13194`, `23194-23197`). Social and generic Nostr events are rejected.
- **Relay only:** It checks signatures and routes by tags, but never decrypts NIP-47 content and stores no wallet connection strings. A Lightning Address service that mints invoices through NWC runs separately and connects to the relay like any other NWC client.

---

## 🔄 How an NWC Payment Flows

<p align="center">
  <img src="docs/nwc-flow.svg" width="960" alt="An NWC payment in six steps: create a connection in Alby Hub, link it to Alby Go, both connect to Mekhala, the app sends an encrypted pay_invoice, the wallet pays over Lightning, and the encrypted result returns to the app.">
</p>

The connection string is handed from the wallet to the app directly, by QR code or copy-paste, and never passes through the relay. Every request and reply after that is end-to-end encrypted between the app and the wallet: Mekhala checks signatures and routes by tags without reading the content.

---

## 📦 Quick Setup

1. **Fork & Deploy:**
   - Fork this repository to your GitHub account.
   - Connect it to your [Cloudflare Dashboard](https://dash.cloudflare.com/) via **Workers & Pages** -> **Create application** -> **Connect to Git** and deploy.
2. **Configure Private Path:**
   - Under your Worker's **Settings** -> **Variables and Secrets**, add a Secret named `RELAY_SECRET`.
   - Set it to a long, secure, unique path identifier.
3. **Attach a Custom Domain:**
   - In **Workers & Pages** → your Worker → **Settings** → **Domains & Routes**, add a Custom Domain (e.g. `relay.example.com`).
   - `wrangler.toml` deploys with `workers_dev = false` and `preview_urls = false`, so the Worker is **not** reachable on `<worker>.<account>.workers.dev`. Set `workers_dev = true` if you cannot attach a domain yet.
4. **Connect Your Wallet:**
   - **Your Relay URL:** `wss://relay.example.com/<your-secret-path>`

---

## ⚙️ Configuration & Limits

Configurable via `wrangler.toml` or environment variables:

| Variable | Default | Description |
|---|---|---|
| `RELAY_SECRET` | *None* | Optional secret path parameter for private routing. |
| `WALLET_REGION` | `apac` | Physical DO location (`apac`, `weur`, `wnam`). |
| `MAX_CONNECTIONS` | `100` | Max concurrent WebSocket connections per DO. When full, the longest-idle connection holding no subscription is closed to free a slot. |
| `MAX_CONNECTIONS_PER_IP` | `5` | Max concurrent connections from one client address; `0` disables. |
| `MAX_CONNECTIONS_PER_ASN` | `20` | Max concurrent connections from one autonomous system; `0` disables. |
| `IDLE_TIMEOUT_SECONDS` | `120` | Close connections idle longer than this that hold no subscription; `0` disables. |
| `CONNECTION_SWEEP_SECONDS` | `60` | How often the idle sweep runs; `0` disables. |
| `EVICT_MIN_IDLE_SECONDS` | `10` | Minimum idle time before a subscription-less connection may be closed to free a slot. |
| `MAX_INFO_EVENTS` | `1000` | Max cached wallet info events (each also costs an `infoid:` key, and each event is limited to 4 KiB); `0` disables. |
| `INFO_TTL_SECONDS` | `2592000` | How long a cached wallet info event is served, measured from its `created_at`; `0` disables. |
| `MESSAGE_RATE_LIMIT_PER_MINUTE` | `120` | Frames a connection may send per minute before it is closed with a `NOTICE`; `0` disables. |
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
