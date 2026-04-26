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
- Click **Add**.
- **Type:** Select **"Secret"** from the dropdown.
- **Name:** Type exactly `RELAY_SECRET`
- **Value:** Paste the secret you copied in Step 1.
- Click **Deploy**.

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
`wss://your-domain.com/<YOUR_SECRET>`

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

## 🇹🇭 ภาษาไทย (Thai)

# nwc-edge-relay ⚡️
**รีเลย์ Nostr ที่เร็ว แรง เป็นส่วนตัว และปลอดภัย สำหรับ Lightning Wallet ของคุณ รันบน Cloudflare Workers**

---

## 🤔 นี่คืออะไร?
หากคุณใช้ **Nostr Wallet Connect (NWC)** เพื่อเชื่อมต่อแอป (เช่น Damus, Amethyst หรือเว็บ zap) เข้ากับ Lightning node ของคุณ (เช่น Alby หรือ Umbrel) คุณจำเป็นต้องมี "รีเลย์" เพื่อให้พวกมันคุยกันได้

ปกติแล้ว รีเลย์ทั่วไปจะเก็บข้อความไว้ในฐานข้อมูล แต่ **nwc-edge-relay** แตกต่างออกไป เพราะมันทำหน้าที่เป็น "ท่อส่งข้อมูลความเร็วสูง" ระหว่างแอปและวอลเล็ตของคุณโดยตรง โดยไม่มีการเก็บข้อมูลใดๆ ซึ่งหมายความว่า:
- 🚀 **เร็วสุดยอด** (ส่งข้อมูลทันที)
- 🔒 **เป็นส่วนตัว 100%** (ข้อมูลของคุณจะไม่ถูกบันทึก)
- 💰 **ฟรีแน่นอน** (รันบน Cloudflare Free Tier ได้สบายๆ)

---

## 📜 NIP ที่รองรับ
- **[NIP-01](https://github.com/nostr-protocol/nips/blob/master/01.md):** โปรโตคอลพื้นฐาน (การเซ็นชื่อ Event, การตรวจสอบ ID, และการรับส่ง REQ/EVENT)
- **[NIP-11](https://github.com/nostr-protocol/nips/blob/master/11.md):** ข้อมูลรีเลย์ (Metadata สำหรับ Client)
- **[NIP-47](https://github.com/nostr-protocol/nips/blob/master/47.md):** Nostr Wallet Connect (การเชื่อมต่อวอลเล็ต)

---

## 📦 การติดตั้งใน 1 คลิก (แนะนำ)
วิธีที่ง่ายที่สุดคือการใช้ระบบ Git Integration ของ Cloudflare

1. **Fork โปรเจกต์นี้:** คลิกปุ่ม **"Fork"** ที่มุมขวาบนของหน้า GitHub นี้เพื่อคัดลอกโปรเจกต์ไปยังบัญชีของคุณ
2. **ล็อกอินเข้า Cloudflare:** ไปที่ [Cloudflare Dashboard](https://dash.cloudflare.com/)
3. **สร้าง Worker:**
   - ไปที่เมนู **Workers & Pages** ทางด้านซ้าย
   - คลิก **Create application** -> **Connect to Git**
   - เลือกโปรเจกต์ `nwc-edge-relay` ที่คุณ Fork ไว้
   - คลิก **Save and Deploy**

*Cloudflare จะใช้เวลาประมาณ 4-5 นาทีในการติดตั้งเครื่องมือและ Build รีเลย์ของคุณเป็นครั้งแรก*

---

## 🔒 การรักษาความปลอดภัย (สำคัญมาก!)
หากคุณไม่ตั้งค่าความปลอดภัย ใครก็ได้บนอินเทอร์เน็ตจะสามารถใช้รีเลย์ของคุณได้ เพื่อความเป็นส่วนตัวและป้องกันโควต้า Cloudflare ของคุณ โปรดทำตามขั้นตอนเหล่านี้:

### 1. สร้างรหัสผ่านลับ (Secret Password)
คุณต้องมีรหัสผ่านแบบสุ่มเพื่อป้องกันรีเลย์ เปิด Terminal ในคอมพิวเตอร์ของคุณ เข้าไปยังโฟลเดอร์ของโปรเจกต์นี้ แล้วรันคำสั่ง:
```bash
./generate_secret.sh
```
*(คัดลอกข้อความรหัสผ่านที่ได้ไว้!)*

### 2. นำรหัสผ่านไปใส่ใน Cloudflare
- ใน Cloudflare Dashboard ไปที่ **Workers & Pages** แล้วคลิกที่ `nwc-edge-relay` ของคุณ
- ไปที่แถบ **Settings** แล้วคลิก **Variables and Secrets**
- คลิก **Add**
- **Type:** เลือก **"Secret"**
- **Name:** พิมพ์ว่า `RELAY_SECRET`
- **Value:** วางรหัสผ่านที่คุณคัดลอกมาจากขั้นตอนที่ 1
- คลิก **Deploy**

---

## 🌍 ตั้งค่าภูมิภาค (ทางเลือก)
เพื่อให้รีเลย์ของคุณเร็วขึ้นไปอีก คุณสามารถบอกให้ Cloudflare รันรีเลย์ในจุดที่ใกล้คุณที่สุดได้ (ค่าเริ่มต้นคือ `apac` หรือเอเชีย)
- ในหน้า **Variables and Secrets** เดิม ภายใต้หัวข้อ **Environment Variables** ให้คลิก **Add variable**
- **Name:** พิมพ์ว่า `WALLET_REGION`
- **Value:** พิมพ์ `apac` (เอเชีย), `weur` (ยุโรป), หรือ `wnam` (อเมริกาตะวันตก)
- คลิก **Deploy**

---

## 🔗 วิธีใช้งานรีเลย์ของคุณ
เมื่อติดตั้งและตั้งค่าความปลอดภัยเรียบร้อยแล้ว คุณสามารถนำ URL นี้ไปใส่ในแอป NWC ได้เลย!

URL รีเลย์ส่วนตัวของคุณจะเป็นดังนี้:
`wss://your-domain.com/<YOUR_SECRET>`

*อย่าลืมเปลี่ยน `<YOUR_SECRET>` เป็นรหัสผ่านที่คุณบันทึกไว้ใน Cloudflare!*

---

## 💻 สำหรับนักพัฒนา (Advanced)
หากคุณต้องการ Build หรือทดสอบในเครื่อง:
- **Build:** `./build.sh`
- **Local Dev:** `npx wrangler dev`
- **Unit Tests:** `cargo test`
- **Integration Tests:** `cd test && npm i && npm test`

### การ Deploy ผ่าน GitHub Actions
คุณสามารถ Deploy ผ่าน GitHub Actions เพื่อความรวดเร็วในการ Build (~30 วินาที) เพียงแค่เพิ่ม `CLOUDFLARE_API_TOKEN` และ `CLOUDFLARE_ACCOUNT_ID` ใน GitHub Repository Secrets แล้ว Push โค้ดไปที่ `main`

---

## ⚖️ License
[MIT License](LICENSE)
