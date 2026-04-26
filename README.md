# nwc-edge-relay ⚡️

**A super fast, private, and secure Nostr relay for your Lightning Wallet, running on Cloudflare Workers.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

---

## 🤔 What is this?

If you use **Nostr Wallet Connect (NWC)** to connect apps (like Damus, Amethyst, or web zaps) to your Lightning node (like Alby or Umbrel), they need a "relay" to talk to each other.

Normally, relays store messages in a database. **nwc-edge-relay** is different. It acts like a direct, high-speed tunnel between your app and your wallet. It doesn't store anything, which means:

- 🚀 **It's blazingly fast** (instant routing)
- 🔒 **It's completely private** (your data is never saved)
- 💰 **It's completely free** (runs comfortably within Cloudflare's free limits)

---

## 📜 Supported NIPs
- **[NIP-01](https://github.com/nostr-protocol/nips/blob/master/01.md):** Basic protocol (Event signing, ID verification, basic REQ/EVENT flow).
- **[NIP-11](https://github.com/nostr-protocol/nips/blob/master/11.md):** Relay Information (JSON metadata for clients).
- **[NIP-47](https://github.com/nostr-protocol/nips/blob/master/47.md):** Nostr Wallet Connect (Info, Request, Response, and Notifications).

---

## 📦 1-Click Deployment (Recommended)

The easiest way to get your own relay running is by using Cloudflare's built-in Git integration.

1. **Fork this repository:** Click the **"Fork"** button at the top right of this GitHub page to copy it to your account.
2. **Log in to Cloudflare:** Go to your [Cloudflare Dashboard](https://dash.cloudflare.com/).
3. **Create the Worker:**
   - Go to **Workers & Pages** in the left sidebar.
   - Click **Create application** -> **Connect to Git**.
   - Select your forked `nwc-edge-relay` repository.
   - Click **Save and Deploy**.

*Cloudflare will take a few minutes to install the necessary tools and build your relay for the first time.*

---

## 🔒 Securing Your Relay (Very Important!)

If you don't secure your relay, anyone on the internet can use it. To keep it private and protect your free Cloudflare limits, follow these simple steps:

### 1. Generate a Secret Password
You need a random password to protect your relay. Open your computer's terminal (or command prompt), navigate to the folder where you cloned this code, and run:
```bash
./generate_secret.sh
```
*(Copy the secret text it gives you!)*

### 2. Add the Password to Cloudflare
- In your Cloudflare Dashboard, go to **Workers & Pages** and click on your `nwc-edge-relay`.
- Go to the **Settings** tab, then click **Variables and Secrets**.
- Under **Secrets**, click **Add secret**.
- **Name:** Type exactly `RELAY_SECRET`
- **Value:** Paste the secret you copied in Step 1.
- Click **Save**.

---

## 🌍 Set Your Region (Optional)

To make your relay even faster, you can tell Cloudflare to put it physically close to your location. This defaults to `apac` (Asia) in `wrangler.toml`.

1. In your Cloudflare Dashboard, go to **Settings** -> **Variables and Secrets**.
2. Under **Environment Variables**, click **Add variable**.
3. **Name:** Type exactly `WALLET_REGION`
4. **Value:** Type `apac` (Asia), `weur` (Europe), or `wnam` (US West).
5. Click **Deploy**.

---

## 🔗 How to Use Your Relay

Now that your relay is deployed and secured, you can use it in your NWC connections!

Your private relay URL will look like this:
`wss://your-relay-name.your-subdomain.workers.dev/<YOUR_SECRET>`

*Just replace `<YOUR_SECRET>` with the password you saved in Cloudflare!*

---

## 💻 For Developers (Advanced)

If you want to build or test locally:
- **Build:** `./build.sh`
- **Local Dev:** `npx wrangler dev`
- **Unit Tests:** `cargo test`
- **Integration Tests:** `cd test && npm i && npm test`

### GitHub Actions Deployment
You can also deploy via GitHub Actions for faster build times (~30 seconds). Just add `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` to your GitHub Repository Secrets and push to `main`.

---

## ⚖️ License
[MIT License](LICENSE)
