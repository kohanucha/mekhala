import { WebSocket } from "ws";
import { RELAY_URL, HTTP_URL, hex, setupTempKV } from "./env.js";
import { nip04, nip44 } from "nostr-tools";
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

export async function testNip44AndFallback() {
  console.log("\n--- Testing NIP-44 Discovery & NIP-04 Fallback ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const nwcSecret = hex(generateSecretKey());
  const nwcUri = `nostr+walletconnect://${walletPk}?relay=${encodeURIComponent(RELAY_URL)}&secret=${nwcSecret}`;

  const handleMakeInvoice = async (walletWs, event, encryptionMethod) => {
    let decrypted;
    if (encryptionMethod === "nip44") {
      const convKey = nip44.getConversationKey(walletSk, event.pubkey);
      decrypted = nip44.decrypt(event.content, convKey);
    } else {
      decrypted = await nip04.decrypt(walletSk, event.pubkey, event.content);
    }

    const req = JSON.parse(decrypted);
    if (req.method === "make_invoice") {
      const resp = JSON.stringify({ result: { invoice: `invoice_${encryptionMethod}` } });
      let encryptedResp;
      if (encryptionMethod === "nip44") {
        const convKey = nip44.getConversationKey(walletSk, event.pubkey);
        encryptedResp = nip44.encrypt(resp, convKey);
      } else {
        encryptedResp = await nip04.encrypt(walletSk, event.pubkey, resp);
      }

      const resEvent = finalizeEvent({
        kind: 23195,
        created_at: Math.floor(Date.now() / 1000),
        tags: [["p", event.pubkey], ["e", event.id]],
        content: encryptedResp
      }, walletSk);
      walletWs.send(JSON.stringify(["EVENT", resEvent]));
    }
  };

  console.log("Step 1: Testing NIP-04 fallback (Info event, no encryption tag)...");
  const walletWs1 = new WebSocket(RELAY_URL);
  await new Promise((resolve, reject) => {
    walletWs1.on("open", () => {
      const infoEvent = finalizeEvent({
        kind: 13194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [],
        content: "make_invoice"
      }, walletSk);
      walletWs1.send(JSON.stringify(["EVENT", infoEvent]));
      walletWs1.send(JSON.stringify(["REQ", "w1", { kinds: [23194], "#p": [walletPk] }]));
    });
    walletWs1.on("message", async (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EOSE") resolve();
      if (msg[0] === "EVENT" && msg[1] === "w1") {
        await handleMakeInvoice(walletWs1, msg[2], "nip04");
      }
    });
    setTimeout(() => reject(new Error("Wallet 1 timeout")), 5000);
  });

  const tempUser = `user_fallback_${Date.now()}`;
  await setupTempKV(tempUser, nwcUri);

  const callbackUrl = `${HTTP_URL.replace(/\/$/, "")}/lnaddress/${tempUser}/callback?amount=21000`;
  const resp1 = await fetch(callbackUrl);
  const data1 = await resp1.json();
  if (data1.pr === "invoice_nip04") {
    console.log("✅ NIP-04 fallback successful.");
  } else {
    throw new Error(`Expected NIP-04 invoice, got: ${JSON.stringify(data1)}`);
  }
  walletWs1.close();

  console.log("Step 2: Testing NIP-44 discovery...");
  const walletWs2 = new WebSocket(RELAY_URL);
  await new Promise((resolve, reject) => {
    walletWs2.on("open", () => {
      const infoEvent = finalizeEvent({
        kind: 13194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [["encryption", "nip44_v2"]],
        content: "pay_invoice make_invoice"
      }, walletSk);
      walletWs2.send(JSON.stringify(["EVENT", infoEvent]));
      walletWs2.send(JSON.stringify(["REQ", "w2", { kinds: [23194, 13194], "#p": [walletPk] }, { kinds: [13194], authors: [walletPk] }]));
    });
    walletWs2.on("message", async (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EOSE") resolve();
      if (msg[0] === "EVENT" && msg[1] === "w2") {
        if (msg[2].kind === 23194) {
          await handleMakeInvoice(walletWs2, msg[2], "nip44");
        }
      }
    });
    setTimeout(() => reject(new Error("Wallet 2 timeout")), 5000);
  });

  const resp2 = await fetch(callbackUrl);
  const data2 = await resp2.json();
  if (data2.pr === "invoice_nip44") {
    console.log("✅ NIP-44 discovery successful.");
  } else {
    throw new Error(`Expected NIP-44 invoice, got: ${JSON.stringify(data2)}`);
  }
  walletWs2.close();
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
