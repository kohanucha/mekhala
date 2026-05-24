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
