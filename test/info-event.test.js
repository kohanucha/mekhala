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

export async function testInfoEventRetrievalWithPtag() {
  console.log("\n--- Realistic Alby Go: #p filter, separate connections ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);

  // Step 1: Wallet service publishes info event and disconnects
  await new Promise((resolve, reject) => {
    const ws1 = new WebSocket(RELAY_URL);
    ws1.on("open", () => {
      const infoEvent = finalizeEvent({
        kind: 13194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [["encryption", "nip44_v2"]],
        content: "pay_invoice make_invoice get_balance get_info",
      }, walletSk);
      ws1.send(JSON.stringify(["EVENT", infoEvent]));
    });
    ws1.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === true) {
        console.log("  ✅ Info event published (OK). Closing wallet connection.");
        ws1.close();
        resolve();
      }
    });
    ws1.on("error", reject);
    setTimeout(() => reject(new Error("Step 1 timeout: publish info event")), 5000);
  });

  // Small delay to ensure WS1 fully closes before WS2 connects
  await new Promise(r => setTimeout(r, 200));

  // Step 2: App (Alby Go) subscribes with #p filter on a fresh connection
  return new Promise((resolve, reject) => {
    const ws2 = new WebSocket(RELAY_URL);
    ws2.on("open", () => {
      console.log("  App connected. Subscribing with #p filter...");
      ws2.send(JSON.stringify(["REQ", "ptag-test", {
        kinds: [13194],
        "#p": [walletPk],
      }]));
    });
    ws2.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      console.log("  Received:", msg[0], msg[1] || "");

      if (msg[0] === "EVENT" && msg[1] === "ptag-test") {
        if (msg[2].kind === 13194) {
          console.log("✅ Info event retrieved via #p filter on separate connection.");
          ws2.close();
          resolve();
        }
      }
    });
    ws2.on("error", reject);
    setTimeout(() => {
      ws2.close();
      reject(new Error(
        "FAIL: Info event via #p filter on separate connection was NOT retrieved. "
        + "This is likely the Alby Go bug — the #p filter matches the event's p-tags "
        + "but info events have none (only encryption tag). "
        + "Check engine.rs process_req for #p filter handling of info events."
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
      appWs.send(JSON.stringify(["REQ", "app-info", { kinds: [13194], "#p": [walletPk] }]));
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
