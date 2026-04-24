# nwc-edge-relay ⚡️

[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)](#)
[![Cloudflare Workers](https://img.shields.io/badge/Cloudflare-Workers-F38020?logo=cloudflare&logoColor=white)](#)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

## Introduction
**nwc-edge-relay** is a specialized, high-performance, and stateless Nostr relay built with **Rust** for **Cloudflare Workers**.

The primary goal is to provide a public, zero-maintenance bridge for **NIP-47 (Nostr Wallet Connect)**. It addresses the overhead of traditional relays by providing an instant, secure, and ephemeral communication channel between wallet applications and Lightning nodes. By routing events in real-time without persistent storage, it serves as a lightweight, specialized infrastructure for the NWC ecosystem.

## Features
- 🚀 **Edge-Native Performance:** Deployed on Cloudflare's global edge network to minimize latency by routing events physically close to users.
- 🪶 **100% Stateless:** Operates entirely in-memory with zero requirements for databases or persistent volumes.
- 🔋 **WebSocket Hibernation:** Optimized resource utilization via Cloudflare's Durable Objects Hibernation API, ensuring high efficiency and low cost.
- 🛡️ **Secure Routing:** Immediate signature verification and encrypted transport for all routed events.
- 📡 **NIP Support:** Fully compliant with **NIP-01** (Basic Protocol), **NIP-11** (Relay Information Document), and **NIP-47** (Nostr Wallet Connect).

## Architecture
**nwc-edge-relay** is designed around the principles of **Stateless Edge Computing**.
When an event is received, the relay performs an instantaneous cryptographic signature check and matches the event against active in-memory subscription filters (such as recipient `#p` tags). If a match is found, the event is forwarded immediately to the subscriber. Because the relay maintains no history, any request for historical data (`REQ`) is immediately met with an `EOSE` (End of Stored Events) message. This architecture maximizes privacy and speed while eliminating operational complexity.

## Public Usage
You can use the public instance of this relay for your Nostr Wallet Connect setups for free!

**Relay URL:**
```
wss://nwc-edge-relay.<YOUR_SUBDOMAIN>.workers.dev
```

**How to use with Alby Hub / Alby Go:**
1. Open your **Alby Hub** or **Alby Go** wallet settings.
2. Navigate to the **Relay** configuration in your connection settings.
3. Update the relay URL to `wss://nwc-edge-relay.<YOUR_SUBDOMAIN>.workers.dev`.
4. Your wallet will now utilize this high-performance edge relay for NWC communication.

## Deploy Your Own
Deploying your own private or public instance on Cloudflare takes only a few minutes.

**Prerequisites:**
- [Node.js](https://nodejs.org/) & npm
- [Rust](https://www.rust-lang.org/) (`rustup target add wasm32-unknown-unknown`)
- [Cloudflare Wrangler](https://developers.cloudflare.com/workers/wrangler/install-and-setup/) (`npm i -g wrangler`)

**Deployment Steps:**

1. **Clone the repository**
   ```bash
   git clone https://github.com/kohanucha/nwc-edge-relay.git
   cd nwc-edge-relay
   ```

2. **Login to Cloudflare**
   ```bash
   wrangler login
   ```

3. **Deploy to the edge**
   The project is configured to automatically set up the Rust environment and build the Wasm binary. Simply run:
   ```bash
   wrangler deploy
   ```
   *(This will trigger `./build.sh` as defined in `wrangler.toml`, which handles Rust installation, target addition, and the build process automatically.)*

## Local Development
Instructions for running and testing the relay in your local environment.

```bash
# Build the project using the build script
./build.sh

# Run the local development server (Miniflare)
wrangler dev

# Run unit tests
cargo test
```

## License
This project is licensed under the [MIT License](LICENSE).
