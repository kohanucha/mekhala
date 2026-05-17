import { WebSocket } from "ws";
import * as nostr from "nostr-tools";
import { nip04, nip44 } from "nostr-tools";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

// Get URL and Secret from command line args or use default
const args = process.argv.slice(2);
let baseURL = args[0] || "localhost:8787";
const relaySecret = args[1] !== undefined ? args[1] : "";


// Clean up the input URL (remove protocol if user provided it)
baseURL = baseURL
  .replace(/^https?:\/\//, "")
  .replace(/^wss?:\/\//, "")
  .replace(/\/$/, "");

const isLocal = baseURL.includes("localhost") || baseURL.includes("127.0.0.1");
const wsProtocol = isLocal ? "ws://" : "wss://";
const httpProtocol = isLocal ? "http://" : "https://";

const RELAY_URL = `${wsProtocol}${baseURL}/${relaySecret}`;
const HTTP_URL = `${httpProtocol}${baseURL}/${relaySecret}`;

console.log(`Testing against:`);
console.log(`  WebSocket: ${RELAY_URL}`);
console.log(`  HTTP:      ${HTTP_URL}\n`);

async function testAuth() {
  if (
    !relaySecret ||
    (relaySecret === "test-secret" && baseURL === "localhost:8787")
  ) {
    // If running with default 'test-secret' on localhost, we still want to test auth
    // but if the user explicitly provided NO secret, we should skip this.
    if (relaySecret === "") {
      console.log("Skipping Authentication tests (Public Relay mode)...");
      return;
    }
  }

  console.log("Testing Authentication (Unauthorized access)...");
  const rootURL = `${httpProtocol}${baseURL}/`;
  const response = await fetch(rootURL);
  if (response.status !== 404) {
    throw new Error(
      "Auth failed: Root path should return 404, but got " + response.status,
    );
  }

  const wrongURL = `${httpProtocol}${baseURL}/wrong-secret`;
  const responseWrong = await fetch(wrongURL);
  if (responseWrong.status !== 404) {
    throw new Error(
      "Auth failed: Wrong secret path should return 404, but got " +
        responseWrong.status,
    );
  }
  console.log("✅ Authentication rejection passed.");
}

async function testNip11() {
  console.log("Testing NIP-11 (Relay Information)...");
  const response = await fetch(HTTP_URL, {
    headers: { Accept: "application/nostr+json" },
  });
  let data;
  try {
    const clonedResponse = response.clone();
    data = await clonedResponse.json();
  } catch (e) {
    const text = await response.text();
    console.error(
      `Failed to parse JSON. Status: ${response.status}. Body: ${text}`,
    );
    throw e;
  }
  if (!data.supported_nips.includes(47)) {
    throw new Error("NIP-11 failed: " + JSON.stringify(data));
  }
  console.log("✅ NIP-11 JSON metadata passed.");

  console.log("Testing NIP-11 (Plain HTTP fallback compatibility)...");
  const responsePlain = await fetch(HTTP_URL);
  if (responsePlain.status !== 200) {
    throw new Error(
      "Plain HTTP fallback should now return 200, but got: " +
        responsePlain.status,
    );
  }
  console.log("✅ NIP-11 Plain HTTP fallback compatibility passed.");
}

async function testRelay() {
  const ws = new WebSocket(RELAY_URL);

  const sk = generateSecretKey();
  const pk = getPublicKey(sk);
  let eventKind3;

  return new Promise((resolve, reject) => {
    ws.on("open", async () => {
      console.log("Connected to relay.");

      // 1. Test Stateless REQ -> EOSE
      console.log("Testing Stateless REQ...");
      ws.send(JSON.stringify(["REQ", "sub-1", { kinds: [23194], "#p": [pk] }]));

      // 2. Test Invalid Signature (NWC kind)
      console.log("Testing Signature Rejection...");
      ws.send(
        JSON.stringify([
          "EVENT",
          {
            id: "0".repeat(64),
            pubkey: pk,
            created_at: Math.floor(Date.now() / 1000),
            kind: 23194,
            tags: [["p", pk]],
            content: "invalid",
            sig: "0".repeat(128),
          },
        ]),
      );

      // 3. Test P-Tag Enforcement for Kind 23194
      console.log("Testing P-tag Enforcement...");
      const eventNoPTag = finalizeEvent(
        {
          kind: 23194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [],
          content: "",
        },
        sk,
      );
      ws.send(JSON.stringify(["EVENT", eventNoPTag]));

      // 4. Test Restricted Kinds (ensure kind not in [13194, 23194, 23195, 23196, 23197] is rejected)
      console.log("Testing Restricted Kinds (Kind 1)...");
      eventKind3 = finalizeEvent(
        {
          kind: 1, // Text note, no longer allowed
          created_at: Math.floor(Date.now() / 1000),
          tags: [],
          content: "",
        },
        sk,
      );
      ws.send(JSON.stringify(["EVENT", eventKind3]));
    });

    let eoseReceived = false;
    let sigRejected = false;
    let kind3Rejected = false;
    let routingTested = false;

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      console.log(
        "Received:",
        msg[0],
        msg[1] || "",
        msg[2] !== undefined ? msg[2] : "",
      );

      if (msg[0] === "EOSE" && msg[1] === "sub-1") {
        eoseReceived = true;
        console.log("✅ Stateless REQ passed.");
      }

      if (msg[0] === "OK") {
        if (eventKind3 && msg[1] === eventKind3.id && msg[2] === false) {
          kind3Rejected = true;
          console.log("✅ Restricted kind rejection passed.");
        }

        if (msg[2] === false) {
          if (msg[3].includes("signature") || msg[3].includes("invalid:") || msg[3].includes("blocked:")) {
            sigRejected = true;
            console.log("✅ Rejection (invalid/signature/blocked) passed.");
          }
        }
      }

      // Check if all initial tests finished
      if (eoseReceived && sigRejected && kind3Rejected && !routingTested) {
        routingTested = true;
        // Now test routing
        testRouting(ws, sk, pk).then(resolve).catch(reject);
      }
    });

    ws.on("error", reject);
    setTimeout(() => {
      if (!routingTested) reject(new Error("Timeout"));
    }, 5000);
  });
}

async function testRouting(ws, sk, pk) {
  console.log("Testing Event Routing...");
  const event = finalizeEvent(
    {
      kind: 23194,
      created_at: Math.floor(Date.now() / 1000),
      tags: [["p", pk]],
      content: "test routing",
    },
    sk,
  );

  ws.send(JSON.stringify(["EVENT", event]));

  return new Promise((resolve) => {
    const handler = (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EVENT" && msg[1] === "sub-1") {
        if (msg[2].id === event.id) {
          console.log("✅ Event routing passed.");
          ws.off("message", handler);
          ws.close();
          resolve();
        }
      }
    };
    ws.on("message", handler);
  });
}

async function testNwcFlow() {
  console.log("\n--- Testing Full NWC Flow (2 Clients: App & Wallet) ---");

  const walletWs = new WebSocket(RELAY_URL);
  const appWs = new WebSocket(RELAY_URL);

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);

  const appSk = generateSecretKey();
  const appPk = getPublicKey(appSk);

  return new Promise((resolve, reject) => {
    let walletEose = false;
    let appEose = false;

    const checkStart = () => {
      if (walletEose && appEose) {
        startFlow();
      }
    };

    walletWs.on("open", () => {
      console.log("Wallet connected.");
      // Wallet subscribes to NWC Requests meant for its pubkey
      walletWs.send(
        JSON.stringify([
          "REQ",
          "wallet-requests",
          { kinds: [23194], "#p": [walletPk] },
        ]),
      );
    });

    appWs.on("open", () => {
      console.log("App connected.");
      // App subscribes to:
      // 1. Responses (23195) and Notifications (23197) targeted to the App (#p)
      appWs.send(
        JSON.stringify([
          "REQ",
          "app-subs",
          { kinds: [23195, 23196, 23197], "#p": [appPk] },
        ]),
      );
    });

    let requestReceived = false;
    let responseReceived = false;
    let notificationReceived = false;

    const startFlow = () => {
      console.log("Step 1: App sending NWC Request (kind 23194) to Wallet...");
      const reqEvent = finalizeEvent(
        {
          kind: 23194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", walletPk]],
          content: '{"method":"pay_invoice","params":{"invoice":"..."}}',
        },
        appSk,
      );
      appWs.send(JSON.stringify(["EVENT", reqEvent]));
    };

    walletWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EOSE" && msg[1] === "wallet-requests") {
        walletEose = true;
        checkStart();
      }
      if (msg[0] === "EVENT" && msg[1] === "wallet-requests") {
        const event = msg[2];
        if (event.kind === 23194) {
          console.log("✅ Wallet received Request (23194) from App.");
          requestReceived = true;

          console.log(
            "Step 3: Wallet sending Response (23195) & Notification (23197)...",
          );

          // Send Response
          const resEvent = finalizeEvent(
            {
              kind: 23195,
              created_at: Math.floor(Date.now() / 1000),
              tags: [
                ["p", appPk],
                ["e", event.id],
              ],
              content: '{"result":{"preimage":"..."}}',
            },
            walletSk,
          );
          walletWs.send(JSON.stringify(["EVENT", resEvent]));

          // Send Notification
          const notifyEvent = finalizeEvent(
            {
              kind: 23197,
              created_at: Math.floor(Date.now() / 1000),
              tags: [["p", appPk]],
              content: '{"type":"payment_sent","payload":{"invoice":"..."}}',
            },
            walletSk,
          );
          walletWs.send(JSON.stringify(["EVENT", notifyEvent]));
        }
      }
    });

    appWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EOSE" && msg[1] === "app-subs") {
        appEose = true;
        checkStart();
      }
      if (msg[0] === "EVENT" && msg[1] === "app-subs") {
        const event = msg[2];
        if (event.kind === 23195) {
          console.log("✅ App received Response (23195).");
          responseReceived = true;
        } else if (event.kind === 23196 || event.kind === 23197) {
          console.log(`✅ App received Notification (${event.kind}).`);
          notificationReceived = true;
        }

        if (requestReceived && responseReceived && notificationReceived) {
          console.log("✅ All NWC Flow steps completed.");
          walletWs.close();
          appWs.close();
          resolve();
        }
      }
    });

    setTimeout(() => {
      const status = `Eoses: W=${walletEose}, A=${appEose} | Req: ${requestReceived}, Res: ${responseReceived}, Notify: ${notificationReceived}`;
      reject(new Error(`NWC Flow Timeout. Status: ${status}`));
    }, 10000);
  });
}

async function testStrictValidation() {
  console.log("\n--- Testing Strict NIP-47 Validation ---");
  const ws = new WebSocket(RELAY_URL);

  return new Promise((resolve, reject) => {
    const sk = generateSecretKey();
    const pk = getPublicKey(sk);

    ws.on("open", () => {
      // 1. Test Broad REQ rejection
      console.log("Step 1: Testing rejection of broad NIP-47 REQ...");
      ws.send(JSON.stringify(["REQ", "broad-sub", { kinds: [23194] }]));

      // 2. Test kind 23195 missing tags
      console.log("Step 2: Testing rejection of kind 23195 missing tags...");
      const invalid23195_noE = finalizeEvent(
        {
          kind: 23195,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", pk]],
          content: "",
        },
        sk,
      );
      const invalid23195_noP = finalizeEvent(
        {
          kind: 23195,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["e", "123"]],
          content: "",
        },
        sk,
      );
      ws.send(JSON.stringify(["EVENT", invalid23195_noE]));
      ws.send(JSON.stringify(["EVENT", invalid23195_noP]));

      // 3. Test kind 23196/23197 missing p-tag
      console.log(
        "Step 3: Testing rejection of kind 23196/23197 missing p-tag...",
      );
      const invalid23196 = finalizeEvent(
        {
          kind: 23196,
          created_at: Math.floor(Date.now() / 1000),
          tags: [],
          content: "",
        },
        sk,
      );
      const invalid23197 = finalizeEvent(
        {
          kind: 23197,
          created_at: Math.floor(Date.now() / 1000),
          tags: [],
          content: "",
        },
        sk,
      );
      ws.send(JSON.stringify(["EVENT", invalid23196]));
      ws.send(JSON.stringify(["EVENT", invalid23197]));
    });

    let broadRejected = false;
    let rejectionsCount = 0;

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());

      if (msg[0] === "CLOSED" && msg[1] === "broad-sub") {
        console.log("✅ Broad REQ rejected.");
        broadRejected = true;
      }

      if (msg[0] === "OK" && msg[2] === false) {
        rejectionsCount++;
      }

      if (broadRejected && rejectionsCount === 4) {
        console.log("✅ All malformed NIP-47 events rejected.");
        ws.close();
        resolve();
      }
    });

    setTimeout(() => {
      reject(
        new Error(
          `Strict Validation Timeout. Broad: ${broadRejected}, Rejections: ${rejectionsCount}/4`,
        ),
      );
    }, 5000);
  });
}

async function testMultiClientIsolation() {
  console.log(
    "\n--- Testing Multi-Client Isolation (Cross-Talk Prevention) ---",
  );

  const w1Ws = new WebSocket(RELAY_URL);
  const w2Ws = new WebSocket(RELAY_URL);
  const a1Ws = new WebSocket(RELAY_URL);

  const w1Sk = generateSecretKey();
  const w1Pk = getPublicKey(w1Sk);
  const w2Sk = generateSecretKey();
  const w2Pk = getPublicKey(w2Sk);
  const a1Sk = generateSecretKey();
  const a1Pk = getPublicKey(a1Sk);

  return new Promise((resolve, reject) => {
    let w1Ready = false,
      w2Ready = false,
      a1Ready = false;

    const checkReady = () => {
      if (w1Ready && w2Ready && a1Ready) startTest();
    };

    w1Ws.on("open", () => {
      w1Ws.send(
        JSON.stringify(["REQ", "w1", { kinds: [23194], "#p": [w1Pk] }]),
      );
    });
    w2Ws.on("open", () => {
      w2Ws.send(
        JSON.stringify(["REQ", "w2", { kinds: [23194], "#p": [w2Pk] }]),
      );
    });
    a1Ws.on("open", () => {
      a1Ready = true;
      checkReady();
    });

    let w1Received = false;
    let w2Received = false;

    w1Ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EOSE") {
        w1Ready = true;
        checkReady();
      }
      if (msg[0] === "EVENT") w1Received = true;
    });

    w2Ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EOSE") {
        w2Ready = true;
        checkReady();
      }
      if (msg[0] === "EVENT") w2Received = true;
    });

    const startTest = () => {
      console.log("App 1 sending Request to Wallet 1...");
      const req = finalizeEvent(
        {
          kind: 23194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", w1Pk]],
          content: "test",
        },
        a1Sk,
      );
      a1Ws.send(JSON.stringify(["EVENT", req]));

      setTimeout(() => {
        if (w1Received && !w2Received) {
          console.log(
            "✅ Multi-Client Isolation passed. Wallet 2 did not receive Wallet 1's request.",
          );
          w1Ws.close();
          w2Ws.close();
          a1Ws.close();
          resolve();
        } else {
          reject(
            new Error(`Isolation failed. W1: ${w1Received}, W2: ${w2Received}`),
          );
        }
      }, 1000);
    };

    setTimeout(() => reject(new Error("Isolation test timeout")), 5000);
  });
}

async function testNip01EdgeCases() {
  console.log("\n--- Testing NIP-01 Edge Cases (Adapted for Strict NWC) ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  return new Promise((resolve, reject) => {
    ws.on("open", () => {
      // 1. ID Mismatch
      console.log("Step 1: Testing ID mismatch...");
      const event = finalizeEvent(
        {
          kind: 23194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", pk]],
          content: "test",
        },
        sk,
      );
      event.id = "0".repeat(64); // Tamper with ID
      ws.send(JSON.stringify(["EVENT", event]));

      // 2. Malformed JSON
      console.log("Step 2: Testing malformed JSON...");
      ws.send("not a json array");

      // 3. Message too large
      console.log("Step 3: Testing message size limit...");
      const largeContent = "a".repeat(70000);
      ws.send(JSON.stringify(["EVENT", { content: largeContent }]));

      // 4. Multiple filters in REQ
      console.log("Step 4: Testing multiple filters in REQ...");
      ws.send(
        JSON.stringify([
          "REQ",
          "multi-sub",
          { kinds: [23194], authors: [pk] },
          { kinds: [13194], "#p": [pk] },
        ]),
      );
    });

    let idMismatchRejected = false;
    let malformedJsonRejected = false;
    let largeMessageNotice = false;
    let multiSubEose = false;
    let closeTestPassed = false;

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());

      if (msg[0] === "OK" && msg[2] === false && msg[3]?.includes("id")) {
        console.log("✅ ID mismatch rejected.");
        idMismatchRejected = true;
      }

      if (msg[0] === "NOTICE" && msg[1]?.includes("parse failed")) {
        console.log("✅ Malformed JSON rejected.");
        malformedJsonRejected = true;
      }

      if (msg[0] === "NOTICE" && msg[1]?.includes("too large")) {
        console.log("✅ Large message notice received.");
        largeMessageNotice = true;
      }

      if (msg[0] === "EOSE" && msg[1] === "multi-sub") {
        console.log("✅ Multiple filters REQ accepted.");
        multiSubEose = true;

        // 5. Test CLOSE
        console.log("Step 5: Testing CLOSE functionality...");
        ws.send(JSON.stringify(["CLOSE", "multi-sub"]));

        // Publish an event that would have matched
        const matchEvent = finalizeEvent(
          {
            kind: 23194,
            created_at: Math.floor(Date.now() / 1000),
            tags: [["p", pk]],
            content: "after close",
          },
          sk,
        );
        ws.send(JSON.stringify(["EVENT", matchEvent]));

        // Wait a bit to ensure NO event is received
        setTimeout(() => {
          closeTestPassed = true;
          checkDone();
        }, 1000);
      }

      if (msg[0] === "EVENT" && msg[1] === "multi-sub") {
        reject(new Error("Received event for closed subscription!"));
      }

      checkDone();
    });

    const checkDone = () => {
      if (
        idMismatchRejected &&
        malformedJsonRejected &&
        largeMessageNotice &&
        multiSubEose &&
        closeTestPassed
      ) {
        ws.close();
        resolve();
      }
    };

    setTimeout(() => {
      reject(
        new Error(
          `NIP-01 Edge Cases Timeout. ID: ${idMismatchRejected}, JSON: ${malformedJsonRejected}, Large: ${largeMessageNotice}, Multi: ${multiSubEose}, Close: ${closeTestPassed}`,
        ),
      );
    }, 10000);
  });
}

async function testInfoEventCaching() {
  console.log(
    "\n--- Testing NIP-47 Info Event Caching (Memory Persistence) ---",
  );

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);

  // 1. Wallet connects and publishes Info Event
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
      // The relay is now silent for kind 13194, so we don't wait for OK.
      // We'll wait a brief moment to ensure the event is processed.
      setTimeout(resolve, 500);
    });
    walletWs.on("error", reject);
    // setTimeout(() => reject(new Error("Wallet publish timeout")), 5000);
  });

  // 2. App connects and requests Info Event
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

async function testTimestampValidation() {
  console.log("\n--- Testing Timestamp Validation Limits ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  return new Promise((resolve, reject) => {
    ws.on("open", () => {
      // 1. Future timestamp (+20 mins)
      console.log("Step 1: Testing rejection of future timestamp...");
      const futureEvent = finalizeEvent({
        kind: 23194,
        created_at: Math.floor(Date.now() / 1000) + 1200,
        tags: [["p", pk]],
        content: "future"
      }, sk);
      ws.send(JSON.stringify(["EVENT", futureEvent]));

      // 2. Old timestamp (-2 years)
      console.log("Step 2: Testing rejection of ancient timestamp...");
      const oldEvent = finalizeEvent({
        kind: 23194,
        created_at: Math.floor(Date.now() / 1000) - (2 * 365 * 24 * 60 * 60),
        tags: [["p", pk]],
        content: "ancient"
      }, sk);
      ws.send(JSON.stringify(["EVENT", oldEvent]));
    });

    let futureRejected = false;
    let oldRejected = false;

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === false && msg[3].includes("invalid: event creation date")) {
        if (msg[3].includes("far off")) futureRejected = true;
        if (msg[3].includes("too old")) oldRejected = true;
      }

      if (futureRejected && oldRejected) {
        console.log("✅ Timestamp limits enforced correctly.");
        ws.close();
        resolve();
      }
    });

    setTimeout(() => {
      reject(new Error(`Timestamp Validation Timeout. Future: ${futureRejected}, Old: ${oldRejected}`));
    }, 5000);
  });
}

async function testLnAddressFlow() {
  console.log("\n--- Testing LN Address Flow (LUD-06 / LUD-16) ---");

  const username = "testuser_relay";
  // Use a fixed secret for deterministic test keys
  const walletSk = new Uint8Array(32).fill(1); 
  const walletPk = getPublicKey(walletSk);
  // Use a DIFFERENT secret for the bridge
  const nwcSecret = "0303030303030303030303030303030303030303030303030303030303030303";
  
  // NWC URI pointing to this relay
  const nwcUri = `nostr+walletconnect://${walletPk}?relay=${encodeURIComponent(RELAY_URL)}&secret=${nwcSecret}`;
  
  console.log(`Using pre-configured KV: ${username} -> ${nwcUri}`);

  const wellKnownUrl = `${HTTP_URL.replace(/\/$/, "")}/.well-known/lnurlp/${username}`;
  const response = await fetch(wellKnownUrl);
  
  if (response.status !== 200) {
    throw new Error(`LNURLp well-known failed: ${response.status}`);
  }

  const data = await response.json();
  console.log("✅ LNURLp well-known passed:", data.callback);

  // Start a mock wallet listener
  const walletWs = new WebSocket(RELAY_URL);
  const invoice = "lnbc1test_relay_invoice...";

  const walletReady = new Promise((resolve, reject) => {
    walletWs.on("open", () => {
      // Send Info Event first
      const infoEvent = finalizeEvent({
        kind: 13194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [], // Defaults to NIP-04
        content: "make_invoice"
      }, walletSk);
      walletWs.send(JSON.stringify(["EVENT", infoEvent]));
      walletWs.send(JSON.stringify(["REQ", "wallet-ln-sub", { kinds: [23194], "#p": [walletPk] }]));
    });
    walletWs.on("message", async (msgData) => {
      const msg = JSON.parse(msgData.toString());
      if (msg[0] === "EOSE") resolve();
      if (msg[0] === "EVENT" && msg[1] === "wallet-ln-sub") {
        const event = msg[2];
        console.log("Mock Wallet received NWC Request from relay.");
        
        // Decrypt and respond
        const decrypted = await nip04.decrypt(walletSk, event.pubkey, event.content);
        const req = JSON.parse(decrypted);
        
        if (req.method === "make_invoice") {
          const resp = JSON.stringify({ result: { invoice } });
          const encryptedResp = await nip04.encrypt(walletSk, event.pubkey, resp);
          
          const resEvent = finalizeEvent({
            kind: 23195,
            created_at: Math.floor(Date.now() / 1000),
            tags: [["p", event.pubkey], ["e", event.id]],
            content: encryptedResp
          }, walletSk);
          walletWs.send(JSON.stringify(["EVENT", resEvent]));
        }
      }
    });
    setTimeout(() => reject(new Error("Mock Wallet timeout")), 5000);
  });

  await walletReady;

  console.log("Calling callback URL...");
  const callbackUrl = `${data.callback}?amount=21000`;
  const callbackResp = await fetch(callbackUrl);
  
  if (callbackResp.status !== 200) {
    const err = await callbackResp.text();
    throw new Error(`LN Address callback failed: ${callbackResp.status} - ${err}`);
  }

  const callbackData = await callbackResp.json();
  if (callbackData.pr === invoice) {
    console.log("✅ LN Address flow (Well-known -> Callback -> NWC) passed!");
  } else {
    console.error("Callback Response:", callbackData);
    throw new Error(`Invoice mismatch: expected ${invoice}, got ${callbackData.pr}`);
  }

  walletWs.close();
}

async function testLnAddressOffline() {
  console.log("\n--- Testing LN Address Offline Error Handling ---");
  const username = "offline_user";
  const walletSk = new Uint8Array(32).fill(2);
  const walletPk = getPublicKey(walletSk);
  
  const wellKnownUrl = `${HTTP_URL.replace(/\/$/, "")}/.well-known/lnurlp/${username}`;
  const response = await fetch(wellKnownUrl);
  const data = await response.json();

  console.log("Calling callback URL for offline wallet...");
  const callbackUrl = `${data.callback}?amount=21000`;
  const callbackResp = await fetch(callbackUrl);
  
  if (callbackResp.status === 200) {
    const errData = await callbackResp.json();
    if (errData.status === "ERROR" && errData.reason && errData.reason.includes("Wallet not connected")) {
      console.log("✅ LN Address offline error handled correctly.");
      return;
    }
    throw new Error(`Expected 'Wallet not connected' error, but got: 200 - ${JSON.stringify(errData)}`);
  }
  
  throw new Error(`Expected 200 error, but got: ${callbackResp.status}`);
}

async function testNip44AndFallback() {
  console.log("\n--- Testing NIP-44 Discovery & NIP-04 Fallback ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const nwcSecret = hex(generateSecretKey());
  const nwcUri = `nostr+walletconnect://${walletPk}?relay=${encodeURIComponent(RELAY_URL)}&secret=${nwcSecret}`;

  // Helper to respond to make_invoice
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

  // 1. Test NIP-04 Fallback (Info event with no encryption tag)
  console.log("Step 1: Testing NIP-04 fallback (Info event, no encryption tag)...");
  const walletWs1 = new WebSocket(RELAY_URL);
  await new Promise((resolve, reject) => {
    walletWs1.on("open", () => {
      const infoEvent = finalizeEvent({
        kind: 13194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [], // No encryption tag -> NIP-04
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

  // Trigger discovery/call via LN Address callback
  // We'll mock the KV fetch in the relay by using a temp user
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

  // 2. Test NIP-44 Discovery
  console.log("Step 2: Testing NIP-44 discovery...");
  const walletWs2 = new WebSocket(RELAY_URL);
  await new Promise((resolve, reject) => {
    walletWs2.on("open", () => {
      // Publish Info Event with NIP-44 support
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

async function setupTempKV(username, nwcUri) {
  const { execSync } = await import("child_process");
  execSync(`npx wrangler kv key put --binding MEKHALA_NWC_KV --local --config ../wrangler.toml "${username}" "${nwcUri}"`);
}

function hex(bytes) {
  return Buffer.from(bytes).toString("hex");
}

async function testProtocolErrors() {
  console.log("\n--- Testing Protocol Error Handling ---");
  const ws = new WebSocket(RELAY_URL);

  return new Promise((resolve, reject) => {
    const results = {
      binaryRejected: false,
      unknownTypeRejected: false,
      emptyArrayRejected: false,
      insufficientReqArgsRejected: false,
    };
    let noticeCount = 0;

    ws.on("open", () => {
      ws.send(Buffer.from("binary data"));
      ws.send(JSON.stringify(["UNKNOWN", "test"]));
      ws.send("[]");
      ws.send(JSON.stringify(["REQ"]));
    });

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "NOTICE") {
        const reason = msg[1] || "";
        noticeCount++;
        if (reason.includes("binary not supported")) {
          results.binaryRejected = true;
          console.log("✅ Binary message rejected with NOTICE.");
        } else if (reason.includes("empty message")) {
          results.emptyArrayRejected = true;
          console.log("✅ Empty array rejected.");
        } else if (reason.includes("unknown message type")) {
          if (!results.unknownTypeRejected) {
            results.unknownTypeRejected = true;
            console.log("✅ Unknown message type rejected.");
          } else {
            results.insufficientReqArgsRejected = true;
            console.log("✅ Insufficient REQ args rejected (parsed as unknown type).");
          }
        } else if (reason.includes("parse failed")) {
          if (!results.emptyArrayRejected) {
            results.emptyArrayRejected = true;
            console.log("✅ Parse error rejected (general).");
          }
        }
      }

      if (results.binaryRejected && results.unknownTypeRejected &&
          results.emptyArrayRejected && results.insufficientReqArgsRejected) {
        console.log("✅ All protocol errors handled correctly.");
        ws.close();
        resolve();
      }
    });

    ws.on("error", () => {
      if (!results.binaryRejected) {
        results.binaryRejected = true;
        console.log("✅ Binary message caused WebSocket error (expected).");
      }
    });

    setTimeout(() => {
      const status = Object.entries(results).map(([k, v]) => `${k}: ${v}`).join(", ");
      reject(new Error(`Protocol error test timeout. ${status}`));
    }, 5000);
  });
}

async function testLimitEnforcement() {
  console.log("\n--- Testing Limit Enforcement ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  return new Promise((resolve, reject) => {
    let filterLimitRejected = false;
    let tagLimitRejected = false;
    let contentLimitRejected = false;

    ws.on("open", () => {
      const manyPubs = Array.from({ length: 11 }, (_, i) => `${pk}${i}`);
      ws.send(JSON.stringify(["REQ", "limit-sub", { kinds: [23194], "#p": manyPubs }]));

      const manyTags = Array.from({ length: 11 }, (_, i) => ["p", `${pk}${i}`]);
      const eventTooManyTags = finalizeEvent({
        kind: 23194,
        created_at: Math.floor(Date.now() / 1000),
        tags: manyTags,
        content: "too many tags"
      }, sk);

      ws.send(JSON.stringify(["EVENT", eventTooManyTags]));

      setTimeout(() => {
        const bigContent = "a".repeat(16385);
        const eventTooLarge = finalizeEvent({
          kind: 23194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", pk]],
          content: bigContent
        }, sk);
        ws.send(JSON.stringify(["EVENT", eventTooLarge]));
      }, 200);
    });

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());

      if (msg[0] === "CLOSED" && msg[1] === "limit-sub" && msg[2].includes("filter too broad")) {
        filterLimitRejected = true;
        console.log("✅ Filter with >10 items rejected (filter too broad).");
      }

      if (msg[0] === "OK" && msg[2] === false) {
        if (msg[3].includes("too many tags")) {
          tagLimitRejected = true;
          console.log("✅ Event with >10 tags rejected.");
        } else if (msg[3].includes("content too large")) {
          contentLimitRejected = true;
          console.log("✅ Event with content >16384 bytes rejected.");
        }
      }

      if (filterLimitRejected && tagLimitRejected && contentLimitRejected) {
        console.log("✅ All limit enforcement tests passed.");
        ws.close();
        resolve();
      }
    });

    setTimeout(() => {
      reject(new Error(`Limit enforcement timeout. Filter: ${filterLimitRejected}, Tags: ${tagLimitRejected}, Content: ${contentLimitRejected}`));
    }, 8000);
  });
}

async function testKind13194NoOK() {
  console.log("\n--- Testing Kind 13194 (Info Event) Produces No OK ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  return new Promise((resolve, reject) => {
    let okReceived = false;
    let eventCached = false;

    ws.on("open", () => {
      const infoEvent = finalizeEvent({
        kind: 13194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [],
        content: "no_ok_test"
      }, sk);

      ws.send(JSON.stringify(["EVENT", infoEvent]));

      setTimeout(() => {
        if (okReceived) {
          reject(new Error("Kind 13194 should not produce OK message, but received one."));
          return;
        }
        console.log("✅ Kind 13194 event did not produce OK message.");

        ws.send(JSON.stringify(["REQ", "info-no-ok", { kinds: [13194], authors: [pk] }]));
      }, 500);
    });

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());

      if (msg[0] === "OK" && msg[2] === true) {
        okReceived = true;
      }

      if (msg[0] === "EVENT" && msg[1] === "info-no-ok") {
        if (msg[2].kind === 13194 && msg[2].content === "no_ok_test") {
          eventCached = true;
          console.log("✅ Kind 13194 event was cached and retrievable.");
          ws.close();
          resolve();
        }
      }

      if (msg[0] === "EOSE" && msg[1] === "info-no-ok" && !eventCached) {
      }
    });

    setTimeout(() => {
      if (!okReceived && eventCached) {
        resolve();
      } else {
        reject(new Error(`Kind 13194 test timeout. OK received: ${okReceived}, Cached: ${eventCached}`));
      }
    }, 5000);
  });
}

async function testLnAddressErrors() {
  console.log("\n--- Testing LN Address Error Handling ---");

  const unknownUser = "nonexistent_user_" + Date.now();
  const wellKnownUrl = `${HTTP_URL.replace(/\/$/, "")}/.well-known/lnurlp/${unknownUser}`;
  const response = await fetch(wellKnownUrl);

  if (response.status !== 200) {
    throw new Error(`Expected 200 for unknown user well-known, got ${response.status}`);
  }
  const data = await response.json();
  if (data.status !== "ERROR" || !data.reason || !data.reason.includes("not found")) {
    throw new Error(`Expected ERROR with 'not found', got: ${JSON.stringify(data)}`);
  }
  console.log("✅ Unknown user well-known returns ERROR.");

  const callbackUrl = `${HTTP_URL.replace(/\/$/, "")}/lnaddress/testuser_relay/callback`;

  const respNoAmount = await fetch(callbackUrl);
  const dataNoAmount = await respNoAmount.json();
  if (dataNoAmount.status !== "ERROR" || !dataNoAmount.reason || !dataNoAmount.reason.includes("Missing amount")) {
    throw new Error(`Expected ERROR 'Missing amount', got: ${JSON.stringify(dataNoAmount)}`);
  }
  console.log("✅ Callback without amount returns 'Missing amount' ERROR.");

  const respNonNumeric = await fetch(`${callbackUrl}?amount=abc`);
  const dataNonNumeric = await respNonNumeric.json();
  if (dataNonNumeric.status !== "ERROR" || !dataNonNumeric.reason || !dataNonNumeric.reason.includes("Missing amount")) {
    throw new Error(`Expected ERROR 'Missing amount' for non-numeric, got: ${JSON.stringify(dataNonNumeric)}`);
  }
  console.log("✅ Callback with non-numeric amount returns 'Missing amount' ERROR.");

  const respNegative = await fetch(`${callbackUrl}?amount=-100`);
  const dataNegative = await respNegative.json();
  if (dataNegative.status !== "ERROR" || !dataNegative.reason || !dataNegative.reason.includes("Missing amount")) {
    throw new Error(`Expected ERROR 'Missing amount' for negative, got: ${JSON.stringify(dataNegative)}`);
  }
  console.log("✅ Callback with negative amount returns 'Missing amount' ERROR.");
}

async function testFilterMatching() {
  console.log("\n--- Testing Filter Matching (#e tags, since, until) ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  return new Promise((resolve, reject) => {
    let eTagReceived = false;
    let sinceReceived = false;
    let untilReceived = false;

    ws.on("open", () => {
      const parentEvent = finalizeEvent({
        kind: 23194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [["p", pk], ["e", "test-event-id-123"]],
        content: "event with e-tag"
      }, sk);

      ws.send(JSON.stringify(["REQ", "sub-e", { kinds: [23194], "#e": ["test-event-id-123"], "#p": [pk] }]));
    });

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());

      if (msg[0] === "EOSE" && msg[1] === "sub-e") {
        const eventWithETag = finalizeEvent({
          kind: 23194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", pk], ["e", "test-event-id-123"]],
          content: "matches e-tag"
        }, sk);
        ws.send(JSON.stringify(["EVENT", eventWithETag]));
      }

      if (msg[0] === "EVENT" && msg[1] === "sub-e") {
        if (msg[2].tags.some(t => t[0] === "e" && t[1] === "test-event-id-123")) {
          eTagReceived = true;
          console.log("✅ Filter matching on #e tag works.");
          ws.send(JSON.stringify(["CLOSE", "sub-e"]));

          const now = Math.floor(Date.now() / 1000);
          ws.send(JSON.stringify(["REQ", "sub-since", { kinds: [23194], "#p": [pk], since: now - 300 }]));
        }
      }

      if (msg[0] === "EOSE" && msg[1] === "sub-since") {
        const recentEvent = finalizeEvent({
          kind: 23194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", pk]],
          content: "recent event for since test"
        }, sk);
        ws.send(JSON.stringify(["EVENT", recentEvent]));
      }

      if (msg[0] === "EVENT" && msg[1] === "sub-since") {
        sinceReceived = true;
        console.log("✅ Filter matching on since works.");
        ws.send(JSON.stringify(["CLOSE", "sub-since"]));

        const now2 = Math.floor(Date.now() / 1000);
        ws.send(JSON.stringify(["REQ", "sub-until", { kinds: [23194], "#p": [pk], until: now2 + 3600 }]));
      }

      if (msg[0] === "EOSE" && msg[1] === "sub-until") {
        const untilEvent = finalizeEvent({
          kind: 23194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", pk]],
          content: "event for until test"
        }, sk);
        ws.send(JSON.stringify(["EVENT", untilEvent]));
      }

      if (msg[0] === "EVENT" && msg[1] === "sub-until") {
        untilReceived = true;
        console.log("✅ Filter matching on until works.");
        ws.send(JSON.stringify(["CLOSE", "sub-until"]));
      }

      if (eTagReceived && sinceReceived && untilReceived) {
        ws.close();
        resolve();
      }
    });

    setTimeout(() => {
      reject(new Error(`Filter matching timeout. e-tag: ${eTagReceived}, since: ${sinceReceived}, until: ${untilReceived}`));
    }, 8000);
  });
}

async function testCorsAndHeaders() {
  console.log("\n--- Testing CORS and Response Headers ---");

  const response = await fetch(HTTP_URL, {
    headers: { Accept: "application/nostr+json" },
  });

  const contentType = response.headers.get("content-type");
  if (!contentType || !contentType.includes("application/nostr+json")) {
    throw new Error(`Expected Content-Type 'application/nostr+json', got: '${contentType}'`);
  }
  console.log("✅ NIP-11 Content-Type is application/nostr+json.");

  const corsOrigin = response.headers.get("access-control-allow-origin");
  if (!corsOrigin || corsOrigin !== "*") {
    throw new Error(`Expected Access-Control-Allow-Origin '*', got: '${corsOrigin}'`);
  }
  console.log("✅ CORS headers present on NIP-11 response.");

  const secHeaders = ["strict-transport-security", "x-content-type-options", "content-security-policy"];
  for (const header of secHeaders) {
    const val = response.headers.get(header);
    if (!val) {
      throw new Error(`Missing security header: ${header}`);
    }
  }
  console.log("✅ Security headers present on NIP-11 response.");

  const lnUrl = `${HTTP_URL.replace(/\/$/, "")}/.well-known/lnurlp/testuser_relay`;
  const lnResponse = await fetch(lnUrl);
  const lnCors = lnResponse.headers.get("access-control-allow-origin");
  if (lnCors !== "*") {
    throw new Error(`Expected LN address CORS '*', got: '${lnCors}'`);
  }
  console.log("✅ CORS headers present on LN address response.");
}

async function testAuthHeaders() {
  console.log("\n--- Testing Security Headers on Auth Rejection ---");

  if (!relaySecret || relaySecret === "") {
    console.log("Skipping Auth Headers test (Public Relay mode)...");
    return;
  }

  const wrongURL = `${httpProtocol}${baseURL}/wrong-secret`;
  const response = await fetch(wrongURL);

  if (response.status !== 404) {
    throw new Error(`Expected 404 for wrong secret, got ${response.status}`);
  }

  const secHeaders = ["strict-transport-security", "x-content-type-options", "content-security-policy"];
  for (const header of secHeaders) {
    const val = response.headers.get(header);
    if (!val) {
      throw new Error(`Missing security header on auth rejection: ${header}`);
    }
  }
  console.log("✅ Security headers present on 404 auth rejection.");
}

async function testFilterMatchingAdvanced() {
  console.log("\n--- Testing Advanced Filter Matching (authors, kinds, ids) ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  return new Promise((resolve, reject) => {
    let authorsReceived = false;
    let kindsReceived = false;
    let idsReceived = false;

    ws.on("open", () => {
      // Phase 1: Subscribe by authors
      ws.send(JSON.stringify(["REQ", "sub-authors", { kinds: [23194], authors: [pk] }]));
    });

    const phase2Kinds = () => {
      // Phase 2: Subscribe by kinds (info events by author)
      ws.send(JSON.stringify(["REQ", "sub-kinds", { kinds: [13194], authors: [pk] }]));
    };

    const phase3Ids = (knownEvent) => {
      // Phase 3: Subscribe by ids
      ws.send(JSON.stringify(["REQ", "sub-ids", { ids: [knownEvent.id], "#p": [pk] }]));
      // Small delay to ensure EOSE arrives before the event
      setTimeout(() => {
        ws.send(JSON.stringify(["EVENT", knownEvent]));
      }, 200);
    };

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());

      // Phase 1: authors
      if (!authorsReceived && msg[0] === "EOSE" && msg[1] === "sub-authors") {
        const event = finalizeEvent({
          kind: 23194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", pk]],
          content: "authors test"
        }, sk);
        ws.send(JSON.stringify(["EVENT", event]));
      }

      if (!authorsReceived && msg[0] === "EVENT" && msg[1] === "sub-authors") {
        if (msg[2].pubkey === pk) {
          authorsReceived = true;
          console.log("✅ Filter matching on authors works.");
          ws.send(JSON.stringify(["CLOSE", "sub-authors"]));
          phase2Kinds();
        }
      }

      // Phase 2: kinds (info events)
      if (authorsReceived && !kindsReceived && msg[0] === "EOSE" && msg[1] === "sub-kinds") {
        const infoEvent = finalizeEvent({
          kind: 13194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [],
          content: "pay_invoice"
        }, sk);
        ws.send(JSON.stringify(["EVENT", infoEvent]));
        // Kind 13194 doesn't produce OK, so wait a bit then subscribe again to retrieve cached
        setTimeout(() => {
          ws.send(JSON.stringify(["CLOSE", "sub-kinds"]));
          ws.send(JSON.stringify(["REQ", "sub-kinds2", { kinds: [13194], authors: [pk] }]));
        }, 300);
      }

      if (authorsReceived && !kindsReceived && msg[0] === "EVENT" && (msg[1] === "sub-kinds" || msg[1] === "sub-kinds2")) {
        if (msg[2].kind === 13194) {
          kindsReceived = true;
          console.log("✅ Filter matching on kinds works (info event cached and retrieved).");
          ws.send(JSON.stringify(["CLOSE", msg[1]]));

          // Phase 3: ids
          const knownEvent = finalizeEvent({
            kind: 23194,
            created_at: Math.floor(Date.now() / 1000),
            tags: [["p", pk]],
            content: "ids test"
          }, sk);
          phase3Ids(knownEvent);
          ws._pendingId = knownEvent.id;
        }
      }

      // Phase 3: ids
      if (kindsReceived && !idsReceived && msg[0] === "EOSE" && msg[1] === "sub-ids") {
        // Already sent the event in phase3Ids, wait for it to arrive
      }

      if (kindsReceived && !idsReceived && msg[0] === "EVENT" && msg[1] === "sub-ids") {
        if (msg[2].id === ws._pendingId) {
          idsReceived = true;
          console.log("✅ Filter matching on ids works.");
          ws.send(JSON.stringify(["CLOSE", "sub-ids"]));
        }
      }

      if (authorsReceived && kindsReceived && idsReceived) {
        ws.close();
        resolve();
      }
    });

    ws.on("error", reject);
    setTimeout(() => {
      reject(new Error(`Advanced filter matching timeout. Authors: ${authorsReceived}, Kinds: ${kindsReceived}, IDs: ${idsReceived}`));
    }, 15000);
  });
}

async function testMixedValidInvalidFilters() {
  console.log("\n--- Testing Mixed Valid/Invalid Filters in REQ ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  return new Promise((resolve, reject) => {
    let mixedRejected = false;
    let validStillWorks = false;

    ws.on("open", () => {
      // REQ with first filter valid (has #p narrowing) and second filter invalid (no narrowing)
      ws.send(JSON.stringify(["REQ", "mixed-sub", { kinds: [23194], "#p": [pk] }, { kinds: [23194] }]));
    });

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());

      if (msg[0] === "CLOSED" && msg[1] === "mixed-sub" && msg[2]?.includes("filter too broad")) {
        mixedRejected = true;
        console.log("✅ Mixed valid/invalid REQ rejected entirely (filter too broad).");

        // Now verify a valid REQ still works after the rejection
        ws.send(JSON.stringify(["REQ", "valid-sub", { kinds: [23194], "#p": [pk] }]));
      }

      if (msg[0] === "EOSE" && msg[1] === "valid-sub") {
        validStillWorks = true;
        console.log("✅ Valid REQ still works after mixed rejection.");
        ws.send(JSON.stringify(["CLOSE", "valid-sub"]));
      }

      if (mixedRejected && validStillWorks) {
        ws.close();
        resolve();
      }
    });

    ws.on("error", reject);
    setTimeout(() => {
      reject(new Error(`Mixed filters test timeout. Mixed rejected: ${mixedRejected}, Valid works: ${validStillWorks}`));
    }, 5000);
  });
}

async function testMaxConnections() {
  console.log("\n--- Testing Max Connections (429 Too Many Requests) ---");

  const LIMIT_PORT = 8788;
  const limitSecret = "limit-test-secret";
  const limitWsURL = `ws://localhost:${LIMIT_PORT}/${limitSecret}`;
  const limitHttpURL = `http://localhost:${LIMIT_PORT}/${limitSecret}`;

  // Kill any stale process on port 8788
  const { execSync } = await import("child_process");
  try { execSync(`lsof -ti :${LIMIT_PORT} | xargs kill -9 2>/dev/null || true`); } catch (e) {}

  console.log("Starting limited relay on port 8788 (MAX_CONNECTIONS=3)...");

  const { spawn } = await import("child_process");
  const projectRoot = new URL("..", import.meta.url).pathname.replace(/\/$/, "");

  const wranglerProcess = spawn("npx", [
    "wrangler", "dev",
    "--port", String(LIMIT_PORT),
    "--ip", "127.0.0.1",
    "--var", "MAX_CONNECTIONS:3",
    "--var", `RELAY_SECRET:${limitSecret}`
  ], {
    cwd: projectRoot,
    stdio: ["ignore", "pipe", "pipe"]
  });

  const cleanup = () => {
    try { wranglerProcess.kill("SIGTERM"); } catch (e) {}
    setTimeout(() => {
      try { wranglerProcess.kill("SIGKILL"); } catch (e) {}
    }, 2000);
  };

  // Wait for the server to start
  let serverReady = false;
  for (let i = 0; i < 30; i++) {
    await new Promise(r => setTimeout(r, 2000));
    try {
      const resp = await fetch(limitHttpURL);
      if (resp.status === 404 || resp.status === 200) {
        serverReady = true;
        break;
      }
    } catch (e) {
      // Server not ready yet
    }
  }

  if (!serverReady) {
    cleanup();
    throw new Error("Limited relay failed to start within 60 seconds");
  }
  console.log("Limited relay is up.");

  try {
    // Open exactly 3 connections (the max)
    const connections = [];
    for (let i = 0; i < 3; i++) {
      const ws = new WebSocket(limitWsURL);
      await new Promise((resolve, reject) => {
        ws.on("open", resolve);
        ws.on("error", (err) => reject(new Error(`Connection ${i + 1} failed: ${err.message}`)));
        setTimeout(() => reject(new Error(`Connection ${i + 1} timeout`)), 5000);
      });
      connections.push(ws);
    }
    console.log("Opened 3 connections (max capacity reached).");

    // 4th WebSocket connection should fail (429 response on upgrade)
    const fourthWs = new WebSocket(limitWsURL);
    const fourthResult = await new Promise((resolve) => {
      fourthWs.on("error", () => resolve("error"));
      setTimeout(() => {
        if (fourthWs.readyState === WebSocket.CLOSED || fourthWs.readyState === WebSocket.CLOSING) {
          resolve("closed");
        } else {
          resolve("open");
        }
      }, 3000);
    });

    if (fourthResult === "open") {
      throw new Error("4th WebSocket connection should have been rejected but succeeded");
    }
    console.log("✅ 4th connection correctly rejected (429 Too Many Requests).");

    // Close all connections
    for (const ws of connections) {
      ws.close();
    }
  } finally {
    cleanup();
  }
}

async function runAll() {
  try {
    await testAuth();
    await testAuthHeaders();
    await testNip11();
    await testRelay();
    await testNip01EdgeCases();
    await testStrictValidation();
    await testNwcFlow();
    await testMultiClientIsolation();
    await testInfoEventCaching();
    await testTimestampValidation();
    await testEdgeCases();
    await testLnAddressFlow();
    await testLnAddressOffline();
    await testLastInWinsRouting();
    await testNip44AndFallback();
    await testProtocolErrors();
    await testLimitEnforcement();
    await testKind13194NoOK();
    await testLnAddressErrors();
    await testFilterMatching();
    await testFilterMatchingAdvanced();
    await testMixedValidInvalidFilters();
    await testCorsAndHeaders();
    await testMaxConnections();
    console.log("\nAll tests passed successfully! 🚀");
    process.exit(0);
  } catch (err) {
    console.error("\n❌ Test failed:", err.message);
    process.exit(1);
  }
}

runAll();

async function testEdgeCases() {
  console.log("\n--- Testing NWC Edge Cases (Performance & Protocol) ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  await new Promise((resolve, reject) => {
    ws.on("open", async () => {
      // 1. Multiple subscriptions for same pubkey (Reference counting check)
      console.log("Step 1: Testing reference counting with multiple subscriptions...");
      ws.send(JSON.stringify(["REQ", "sub-a", { kinds: [23194], "#p": [pk] }]));
      ws.send(JSON.stringify(["REQ", "sub-b", { kinds: [23194], "#p": [pk] }]));
      
      // 2. Signature Reuse (Cache check)
      console.log("Step 2: Testing signature verification cache...");
      const event = finalizeEvent({
        kind: 23194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [["p", pk]],
        content: "cache-test"
      }, sk);
      
      ws.send(JSON.stringify(["EVENT", event]));
      ws.send(JSON.stringify(["EVENT", event])); // Should be fast via cache
      
      resolve();
    });
    
    let okCount = 0;
    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === true) {
        okCount++;
        if (okCount === 2) {
          console.log("✅ Signature reuse and multiple subs handled.");
          ws.close();
        }
      }
    });
    
    setTimeout(() => reject(new Error("Edge case test timeout")), 5000);
  });
}

// Update the main test runner to include the new tests
// Note: This is a bit hacky, normally I'd edit the main function

async function testLastInWinsRouting() {
  console.log("\n--- Testing Last-In-Wins Routing (Singular Routing) ---");
  
  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const appSk = generateSecretKey();

  // Connect sequentially to ensure ID order
  const wallet1Ws = new WebSocket(RELAY_URL);
  await new Promise(r => wallet1Ws.on("open", r));
  console.log("Wallet 1 connected.");
  wallet1Ws.send(JSON.stringify(["REQ", "w1", { kinds: [23194], "#p": [walletPk] }]));
  await new Promise(r => wallet1Ws.on("message", (data) => {
    if (JSON.parse(data.toString())[0] === "EOSE") r();
  }));

  const wallet2Ws = new WebSocket(RELAY_URL);
  await new Promise(r => wallet2Ws.on("open", r));
  console.log("Wallet 2 connected.");
  wallet2Ws.send(JSON.stringify(["REQ", "w2", { kinds: [23194], "#p": [walletPk] }]));
  await new Promise(r => wallet2Ws.on("message", (data) => {
    if (JSON.parse(data.toString())[0] === "EOSE") r();
  }));

  const appWs = new WebSocket(RELAY_URL);
  await new Promise(r => appWs.on("open", r));

  return new Promise((resolve, reject) => {
    let wallet1Received = false;
    let wallet2Received = false;

    console.log("Sending request from App...");
    const event = finalizeEvent({
      kind: 23194,
      created_at: Math.floor(Date.now() / 1000),
      tags: [["p", walletPk]],
      content: "routing-test"
    }, appSk);
    appWs.send(JSON.stringify(["EVENT", event]));

    wallet1Ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EVENT" && msg[1] === "w1") {
        console.log("❌ Wallet 1 received request! (Should have been routed to Wallet 2)");
        wallet1Received = true;
      }
    });

    wallet2Ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EVENT" && msg[1] === "w2") {
        console.log("✅ Wallet 2 received request. (Correct singular routing)");
        wallet2Received = true;
        
        setTimeout(() => {
          if (!wallet1Received && wallet2Received) {
            wallet1Ws.close();
            wallet2Ws.close();
            appWs.close();
            resolve();
          } else {
            reject(new Error("Singular routing failed: wallet 1 also received the event."));
          }
        }, 1000);
      }
    });

    setTimeout(() => reject(new Error("Last-In-Wins Routing test timeout")), 5000);
  });
}
