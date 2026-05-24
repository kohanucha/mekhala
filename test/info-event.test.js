import { WebSocket } from "ws";
import { RELAY_URL } from "./env.js";
import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";

export async function testInfoEventRetrieval() {
  console.log("\n--- Simulating Alby Go: Wallet Info Event (kind 13194) Retrieval ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const ws = new WebSocket(RELAY_URL);

  return new Promise((resolve, reject) => {
    let infoEventId = null;

    ws.on("open", () => {
      console.log("  Connected. Publishing info event (kind 13194)...");

      const infoEvent = finalizeEvent({
        kind: 13194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [["encryption", "nip44_v2"]],
        content: "pay_invoice make_invoice get_balance get_info",
      }, walletSk);
      infoEventId = infoEvent.id;
      ws.send(JSON.stringify(["EVENT", infoEvent]));

      ws.send(JSON.stringify(["REQ", "info-test", {
        kinds: [13194],
        authors: [walletPk],
      }]));
    });

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      console.log("  Received:", msg[0], msg[1] || "");

      if (msg[0] === "OK" && msg[1] === infoEventId) {
        console.log("  ✅ Info event published (OK).");
      }

      if (msg[0] === "EVENT" && msg[1] === "info-test") {
        if (msg[2].kind === 13194) {
          console.log("✅ Alby Go simulation: info event (kind 13194) retrieved successfully.");
          ws.close();
          resolve();
        }
      }

      if (msg[0] === "EOSE" && msg[1] === "info-test") {
        console.log("  EOSE received (subscription active).");
      }
    });

    ws.on("error", reject);
    setTimeout(() => {
      ws.close();
      reject(new Error(
        "FAIL: Info event (kind 13194) was NOT retrieved. "
        + "Publish succeeded (OK) but REQ returned no EVENT. "
        + "Check: wallet registry caching, filter matching, or subscription routing."
      ));
    }, 5000);
  });
}

export async function testRealisticNwcFlow() {
  console.log("\n--- Realistic NWC Flow: Wallet + App via #p discovery ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const appSk = generateSecretKey();
  const appPk = getPublicKey(appSk);

  const TIMEOUT = 10000;

  // Step 1: Wallet connects, publishes info, subscribes for NWC requests
  const walletReady = new Promise((resolve, reject) => {
    const ws = new WebSocket(RELAY_URL);
    const sentinel = setTimeout(() => { ws.close(); reject(new Error("Wallet setup timeout")); }, TIMEOUT);
    ws.on("open", () => {
      const info = finalizeEvent({
        kind: 13194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [["encryption", "nip44_v2"]],
        content: "pay_invoice make_invoice get_balance get_info",
      }, walletSk);
      ws.send(JSON.stringify(["EVENT", info]));
      ws.send(JSON.stringify(["REQ", "wallet-nwc", { kinds: [23194], "#p": [walletPk] }]));
    });
    let haveInfoOk = false;
    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === true) {
        haveInfoOk = true;
      }
      if (msg[0] === "EOSE" && msg[1] === "wallet-nwc" && haveInfoOk) {
        console.log("  ✅ Wallet: info published + NWC subscription active.");
        clearTimeout(sentinel);
        resolve(ws);
      }
    });
    ws.on("error", (e) => { clearTimeout(sentinel); reject(e); });
  });

  const walletWs = await walletReady;

  // Step 2: App connects, subscribes for info + responses, discovers wallet
  const appWs = new WebSocket(RELAY_URL);

  const appDiscovered = new Promise((resolve, reject) => {
    const sentinel = setTimeout(() => { appWs.close(); reject(new Error("App discovery timeout")); }, TIMEOUT);
    appWs.on("open", () => {
      // Subscribe for info event discovery AND response subscription upfront
      appWs.send(JSON.stringify(["REQ", "app-info", { kinds: [13194], authors: [walletPk] }]));
      appWs.send(JSON.stringify(["REQ", "app-resp", { kinds: [23195], "#p": [appPk] }]));
    });
    appWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      // Handle info event discovery — send NWC request
      if (msg[0] === "EVENT" && msg[1] === "app-info" && msg[2].kind === 13194) {
        clearTimeout(sentinel);
        console.log("  ✅ App: discovered wallet via #p filter.");
        const req = finalizeEvent({
          kind: 23194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", walletPk]],
          content: '{"method":"pay_invoice","params":{"invoice":"lnbc1..."}}',
        }, appSk);
        appWs.send(JSON.stringify(["EVENT", req]));
        console.log("  ✅ App: sent NWC request (kind 23194).");
        resolve();
      }
    });
    appWs.on("error", (e) => { clearTimeout(sentinel); reject(e); });
  });

  // Step 3: Wallet receives NWC request, sends response
  await appDiscovered;
  await new Promise((resolve, reject) => {
    const sentinel = setTimeout(() => reject(new Error("Wallet: no NWC request received")), TIMEOUT);
    walletWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EVENT" && msg[1] === "wallet-nwc" && msg[2].kind === 23194) {
        const reqEvent = msg[2];
        clearTimeout(sentinel);
        console.log("  ✅ Wallet: received NWC request from App.");
        const resp = finalizeEvent({
          kind: 23195,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", appPk], ["e", reqEvent.id]],
          content: '{"result":{"preimage":"abc123"}}',
        }, walletSk);
        walletWs.send(JSON.stringify(["EVENT", resp]));
        console.log("  ✅ Wallet: sent NWC response (kind 23195).");
        resolve();
      }
    });
  });

  // Step 4: App receives the response on the "app-resp" subscription
  await new Promise((resolve, reject) => {
    const sentinel = setTimeout(() => reject(new Error("App: no NWC response received")), TIMEOUT);
    appWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EVENT" && msg[1] === "app-resp" && msg[2].kind === 23195) {
        clearTimeout(sentinel);
        console.log("  ✅ App: received NWC response (kind 23195).");
        resolve();
      }
    });
  });

  appWs.close();
  walletWs.close();
  console.log("✅ Realistic NWC flow completed successfully.");
}

export async function testRealisticPaymentFlow() {
  console.log("\n--- Realistic Payment Flow: send (pay_invoice) + receive (make_invoice) ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const aliceSk = generateSecretKey();
  const alicePk = getPublicKey(aliceSk);
  const bobSk = generateSecretKey();
  const bobPk = getPublicKey(bobSk);

  const TIMEOUT = 10000;

  // Step 1: Wallet — publish info, subscribe for NWC requests
  const walletReady = new Promise((resolve, reject) => {
    const ws = new WebSocket(RELAY_URL);
    const sentinel = setTimeout(() => { ws.close(); reject(new Error("Wallet setup timeout")); }, TIMEOUT);
    ws.on("open", () => {
      const info = finalizeEvent({
        kind: 13194, created_at: Math.floor(Date.now() / 1000),
        tags: [["encryption", "nip44_v2"]],
        content: "pay_invoice make_invoice get_balance get_info",
      }, walletSk);
      ws.send(JSON.stringify(["EVENT", info]));
      ws.send(JSON.stringify(["REQ", "wallet-req", { kinds: [23194], "#p": [walletPk] }]));
    });
    let haveOk = false;
    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === true) haveOk = true;
      if (msg[0] === "EOSE" && msg[1] === "wallet-req" && haveOk) {
        clearTimeout(sentinel); resolve(ws);
      }
    });
    ws.on("error", (e) => { clearTimeout(sentinel); reject(e); });
  });
  const walletWs = await walletReady;
  console.log("  ✅ Wallet: info published + request subscription active.");

  // Connect an app, discover wallet via #p, send NWC request
  // Returns { ws, subId, response, notification } with response/notification
  // as promises that resolve when the respective EVENT arrives.
  async function connectApp(appSk, appPk, method, params) {
    const ws = new WebSocket(RELAY_URL);
    const subId = `sub-${method}`;

    let resolveResp, resolveNotify;
    const response = new Promise(r => { resolveResp = r; });
    const notification = new Promise(r => { resolveNotify = r; });

    const setup = new Promise((resolve, reject) => {
      const sentinel = setTimeout(() => { ws.close(); reject(new Error(`${method} setup timeout`)); }, TIMEOUT);
      ws.on("open", () => {
        ws.send(JSON.stringify(["REQ", `info-${method}`, { kinds: [13194], authors: [walletPk] }]));
        ws.send(JSON.stringify(["REQ", subId, { kinds: [23195, 23196, 23197], "#p": [appPk] }]));
      });
      ws.on("message", (data) => {
        const msg = JSON.parse(data.toString());
        if (msg[0] === "EVENT" && msg[1] === subId) {
          if (msg[2].kind === 23195) resolveResp(msg[2]);
          if (msg[2].kind === 23197) resolveNotify(msg[2]);
        }
        if (msg[0] === "EVENT" && msg[1] === `info-${method}` && msg[2].kind === 13194) {
          clearTimeout(sentinel);
          const req = finalizeEvent({
            kind: 23194, created_at: Math.floor(Date.now() / 1000),
            tags: [["p", walletPk]],
            content: JSON.stringify({ method, params }),
          }, appSk);
          ws.send(JSON.stringify(["EVENT", req]));
          console.log(`  ✅ ${method}: discovered wallet → sent request.`);
          resolve();
        }
      });
      ws.on("error", (e) => { clearTimeout(sentinel); reject(e); });
    });
    await setup;
    return { ws, subId, response, notification };
  }

  // ---- SEND PAYMENT: Alice pays an invoice ----
  console.log("\n  --- Send Payment (pay_invoice) ---");
  const alice = await connectApp(aliceSk, alicePk, "pay_invoice", { invoice: "lnbc1n1p_test_invoice" });

  // Wallet processes Alice's request
  let aliceReqId;
  await new Promise((resolve, reject) => {
    const sentinel = setTimeout(() => reject(new Error("Wallet: no pay_invoice request")), TIMEOUT);
    walletWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EVENT" && msg[1] === "wallet-req" && msg[2].kind === 23194) {
        const event = msg[2];
        aliceReqId = event.id;
        clearTimeout(sentinel);

        const preimage = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        const resp = finalizeEvent({
          kind: 23195, created_at: Math.floor(Date.now() / 1000),
          tags: [["p", alicePk], ["e", event.id]],
          content: JSON.stringify({ result: { preimage } }),
        }, walletSk);
        walletWs.send(JSON.stringify(["EVENT", resp]));

        const notify = finalizeEvent({
          kind: 23197, created_at: Math.floor(Date.now() / 1000),
          tags: [["p", alicePk]],
          content: JSON.stringify({ type: "payment_sent", payload: { preimage } }),
        }, walletSk);
        walletWs.send(JSON.stringify(["EVENT", notify]));

        console.log("  ✅ Wallet: processed pay_invoice → response + notification sent.");
        resolve();
      }
    });
  });

  // Verify Alice's response
  const aliceResp = await alice.response;
  if (!aliceResp.tags.some(t => t[0] === "e" && t[1] === aliceReqId)) {
    alice.ws.close(); walletWs.close();
    throw new Error("Alice pay_invoice response missing e-tag");
  }
  console.log("  ✅ Alice: received pay_invoice response with valid e-tag.");

  // Verify Alice's notification
  const aliceNotify = await alice.notification;
  if (aliceNotify.kind !== 23197) {
    alice.ws.close(); walletWs.close();
    throw new Error("Alice notification has wrong kind");
  }
  console.log("  ✅ Alice: received payment notification (kind 23197).");

  // ---- RECEIVE PAYMENT: Bob requests an invoice ----
  console.log("\n  --- Receive Payment (make_invoice) ---");
  const bob = await connectApp(bobSk, bobPk, "make_invoice", { amount: 21000 });

  let bobReqId;
  await new Promise((resolve, reject) => {
    const sentinel = setTimeout(() => reject(new Error("Wallet: no make_invoice request")), TIMEOUT);
    walletWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EVENT" && msg[1] === "wallet-req" && msg[2].kind === 23194) {
        const event = msg[2];
        bobReqId = event.id;
        clearTimeout(sentinel);
        const resp = finalizeEvent({
          kind: 23195, created_at: Math.floor(Date.now() / 1000),
          tags: [["p", bobPk], ["e", event.id]],
          content: JSON.stringify({ result: { invoice: "lnbc210n1p_test_invoice", payment_hash: "abcdef0123456789abcdef0123456789" } }),
        }, walletSk);
        walletWs.send(JSON.stringify(["EVENT", resp]));
        console.log("  ✅ Wallet: processed make_invoice → response sent.");
        resolve();
      }
    });
  });

  const bobResp = await bob.response;
  if (!bobResp.tags.some(t => t[0] === "e" && t[1] === bobReqId)) {
    alice.ws.close(); bob.ws.close(); walletWs.close();
    throw new Error("Bob make_invoice response missing e-tag");
  }
  console.log("  ✅ Bob: received make_invoice response with valid e-tag.");

  alice.ws.close();
  bob.ws.close();
  walletWs.close();
  console.log("✅ Realistic payment flow completed successfully.");
}
