import { WebSocket } from "ws";
import { RELAY_URL } from "./env.js";
import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";

export async function testInfoEventCaching() {
  console.log(
    "\n--- Testing NIP-47 Info Event Caching (Memory Persistence) ---",
  );

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);

  const walletWs = new WebSocket(RELAY_URL);
  await new Promise((resolve, reject) => {
    walletWs.on("open", () => {
      const infoEvent = finalizeEvent(
        {
          kind: 13194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [],
          content: "cached_info_test",
        },
        walletSk,
      );
      walletWs.send(JSON.stringify(["EVENT", infoEvent]));
    });

    walletWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === true) {
        console.log("✅ Wallet received OK for Info Event (13194).");
        resolve();
      }
    });

    walletWs.on("error", reject);
    setTimeout(() => reject(new Error("Wallet publish timeout waiting for OK")), 5000);
  });

  const appWs = new WebSocket(RELAY_URL);
  await new Promise((resolve, reject) => {
    appWs.on("open", () => {
      appWs.send(
        JSON.stringify([
          "REQ",
          "cache-sub",
          { kinds: [13194], authors: [walletPk] },
        ]),
      );
    });

    appWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EVENT" && msg[1] === "cache-sub") {
        if (msg[2].kind === 13194 && msg[2].content === "cached_info_test") {
          console.log("✅ App received cached Info Event (13194) correctly.");
          appWs.close();
          resolve();
        }
      }
    });

    appWs.on("error", reject);
    setTimeout(() => reject(new Error("App cache retrieval timeout")), 5000);
  });

  walletWs.close();
}

export async function testKind13194OK() {
  console.log("\n--- Testing Kind 13194 (Info Event) Produces OK and Broadcast ---");
  const ws = new WebSocket(RELAY_URL);
  const subWs = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  return new Promise((resolve, reject) => {
    let okReceived = false;
    let eventBroadcastReceived = false;
    let eventCached = false;

    subWs.on("open", () => {
      subWs.send(JSON.stringify(["REQ", "sub-13194", { kinds: [13194], authors: [pk] }]));
    });

    subWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EVENT" && msg[1] === "sub-13194") {
        if (msg[2].kind === 13194 && msg[2].content === "ok_test") {
          eventBroadcastReceived = true;
          console.log("✅ Kind 13194 broadcast received by subscriber.");
        }
      }
    });

    ws.on("open", () => {
      setTimeout(() => {
        const infoEvent = finalizeEvent({
          kind: 13194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [],
          content: "ok_test"
        }, sk);

        ws.send(JSON.stringify(["EVENT", infoEvent]));
      }, 500);
    });

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());

      if (msg[0] === "OK" && msg[2] === true) {
        okReceived = true;
        console.log("✅ Kind 13194 event produced OK message.");
        ws.send(JSON.stringify(["REQ", "info-ok", { kinds: [13194], authors: [pk] }]));
      }

      if (msg[0] === "EVENT" && msg[1] === "info-ok") {
        if (msg[2].kind === 13194 && msg[2].content === "ok_test") {
          eventCached = true;
          console.log("✅ Kind 13194 event was cached and retrievable.");
        }
      }

      if (okReceived && eventCached && eventBroadcastReceived) {
        ws.close();
        subWs.close();
        resolve();
      }
    });

    setTimeout(() => {
      ws.close();
      subWs.close();
      reject(new Error(`Kind 13194 test timeout. OK: ${okReceived}, Cached: ${eventCached}, Broadcast: ${eventBroadcastReceived}`));
    }, 8000);
  });
}

/// Kind 13194 is replaceable: sending an older info event again, which anyone
/// who saw it can do, must not roll the cache back.
export async function testInfoEventNotRolledBack() {
  console.log("\n--- Testing an older info event does not replace a newer one ---");
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);
  const now = Math.floor(Date.now() / 1000);
  const older = finalizeEvent({ kind: 13194, created_at: now - 60, tags: [], content: "pay_invoice" }, sk);
  const newer = finalizeEvent({ kind: 13194, created_at: now, tags: [], content: "pay_invoice get_balance" }, sk);
  const ws = new WebSocket(RELAY_URL);

  const send = (event) => new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`no OK for ${event.id.slice(0, 8)}`)), 5000);
    const handler = (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[1] === event.id) {
        clearTimeout(timer);
        ws.off("message", handler);
        resolve();
      }
    };
    ws.on("message", handler);
    ws.send(JSON.stringify(["EVENT", event]));
  });

  const cached = () => new Promise((resolve, reject) => {
    const ids = [];
    const timer = setTimeout(() => reject(new Error("no EOSE")), 5000);
    const handler = (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[1] !== "rollback") return;
      if (msg[0] === "EVENT") ids.push(msg[2].id);
      if (msg[0] === "EOSE") {
        clearTimeout(timer);
        ws.off("message", handler);
        resolve(ids);
      }
    };
    ws.on("message", handler);
    ws.send(JSON.stringify(["REQ", "rollback", { kinds: [13194], authors: [pk] }]));
  });

  await new Promise((resolve, reject) => {
    ws.on("open", resolve);
    ws.on("error", reject);
  });
  try {
    await send(older);
    await send(newer);
    await send(older);
    const ids = await cached();
    if (ids.length !== 1 || ids[0] !== newer.id) {
      throw new Error(`expected the newer info event, got ${JSON.stringify(ids)}`);
    }
    console.log("✅ The older info event sent again did not replace the newer one.");
  } finally {
    ws.close();
  }
}
