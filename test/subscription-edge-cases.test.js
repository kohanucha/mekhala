import { WebSocket } from "ws";
import { RELAY_URL } from "./env.js";
import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";

export async function testCloseNonExistentSub() {
  console.log("\n--- Testing CLOSE on Non-Existent Subscription ---");
  const ws = new WebSocket(RELAY_URL);
  return new Promise((resolve, reject) => {
    ws.on("open", () => {
      ws.send(JSON.stringify(["CLOSE", "never-created-sub"]));
    });
    setTimeout(() => {
      console.log("✅ CLOSE on non-existent sub handled (no error).");
      ws.close();
      resolve();
    }, 2000);
    ws.on("error", reject);
  });
}

export async function testReplaceSubscription() {
  console.log("\n--- Testing Replace Subscription (same sub_id) ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  return new Promise((resolve, reject) => {
    let eoseCount = 0;
    let eventsAfterReplace = 0;

    ws.on("open", () => {
      ws.send(JSON.stringify(["REQ", "replace-sub", { kinds: [23194], "#p": [pk] }]));
    });

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());

      if (msg[0] === "EOSE" && msg[1] === "replace-sub") {
        eoseCount++;
        if (eoseCount === 1) {
          ws.send(JSON.stringify(["REQ", "replace-sub", { kinds: [23194], authors: [pk] }]));
        } else if (eoseCount === 2) {
          const eventWithPTag = finalizeEvent({
            kind: 23194,
            created_at: Math.floor(Date.now() / 1000),
            tags: [["p", pk]],
            content: "after-replace",
          }, sk);
          ws.send(JSON.stringify(["EVENT", eventWithPTag]));
        }
      }

      if (msg[0] === "EVENT" && msg[1] === "replace-sub") {
        eventsAfterReplace++;
      }

      if (eoseCount === 2) {
        setTimeout(() => {
          if (eventsAfterReplace === 1) {
            console.log("✅ Subscription replaced correctly (new filter active).");
            ws.close();
            resolve();
          } else {
            reject(new Error(`Expected 1 event after replace, got ${eventsAfterReplace}`));
          }
        }, 1000);
      }
    });

    ws.on("error", reject);
    setTimeout(() => reject(new Error("Replace subscription test timeout")), 5000);
  });
}

export async function testDuplicateEventPublish() {
  console.log("\n--- Testing Duplicate Event Publish (no dedup) ---");
  const appSk = generateSecretKey();
  const appPk = getPublicKey(appSk);
  const subWs = new WebSocket(RELAY_URL);
  const pubWs = new WebSocket(RELAY_URL);

  return new Promise((resolve, reject) => {
    let subOk = false;
    let eventCount = 0;

    subWs.on("open", () => {
      subWs.send(JSON.stringify(["REQ", "dedup-sub", { kinds: [23194], "#p": [appPk] }]));
    });

    subWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EOSE" && msg[1] === "dedup-sub") {
        subOk = true;
        if (pubWs.readyState === WebSocket.OPEN) publishTwice();
      }
      if (msg[0] === "EVENT" && msg[1] === "dedup-sub") {
        eventCount++;
      }
    });

    let publishedTwice = false;
    const publishTwice = () => {
      const event = finalizeEvent({
        kind: 23194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [["p", appPk]],
        content: "dedup-test",
      }, appSk);

      pubWs.send(JSON.stringify(["EVENT", event]));
      pubWs.send(JSON.stringify(["EVENT", event]));
      publishedTwice = true;
    };

    pubWs.on("open", () => {
      if (subOk) publishTwice();
    });

    let okCount = 0;
    pubWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === true) {
        okCount++;
      }
    });

    setTimeout(() => {
      if (!publishedTwice) {
        reject(new Error("Never published"));
        return;
      }
      if (okCount !== 2) {
        reject(new Error(`Publisher got ${okCount}/2 OKs`));
        return;
      }
      if (eventCount === 2) {
        console.log("✅ Duplicate event accepted twice (no dedup).");
        subWs.close();
        pubWs.close();
        resolve();
      } else {
        reject(new Error(`Subscriber got ${eventCount}/2 events`));
      }
    }, 2000);

    subWs.on("error", reject);
    pubWs.on("error", reject);
    setTimeout(() => reject(new Error("Duplicate event test timeout")), 5000);
  });
}
