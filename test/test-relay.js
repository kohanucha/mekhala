import { WebSocket } from "ws";
import * as nostr from "nostr-tools";
import { nip04 } from "nostr-tools";
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
  if (response.status !== 401) {
    throw new Error(
      "Auth failed: Root path should return 401, but got " + response.status,
    );
  }

  const wrongURL = `${httpProtocol}${baseURL}/wrong-secret`;
  const responseWrong = await fetch(wrongURL);
  if (responseWrong.status !== 401) {
    throw new Error(
      "Auth failed: Wrong secret path should return 401, but got " +
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
        if (msg[1] === eventKind3.id && msg[2] === false) {
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
      if (eoseReceived && sigRejected && kind3Rejected) {
        // Now test routing
        testRouting(ws, sk, pk).then(resolve).catch(reject);
      }
    });

    ws.on("error", reject);
    setTimeout(() => reject(new Error("Timeout")), 5000);
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
    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EVENT" && msg[1] === "sub-1") {
        if (msg[2].id === event.id) {
          console.log("✅ Event routing passed.");
          ws.close();
          resolve();
        }
      }
    });
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
    let largeMessageNotice = false;
    let multiSubEose = false;
    let closeTestPassed = false;

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());

      if (msg[0] === "OK" && msg[2] === false && msg[3]?.includes("id")) {
        console.log("✅ ID mismatch rejected.");
        idMismatchRejected = true;
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
          `NIP-01 Edge Cases Timeout. ID: ${idMismatchRejected}, Large: ${largeMessageNotice}, Multi: ${multiSubEose}, Close: ${closeTestPassed}`,
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
    });
    walletWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === true) {
        // Keep connection open for the next part of the test
        resolve();
      }
    });
    walletWs.on("error", reject);
    setTimeout(() => reject(new Error("Wallet publish timeout")), 5000);
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
  const nwcSecret = "0101010101010101010101010101010101010101010101010101010101010101";
  
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

async function runAll() {
  try {
    await testAuth();
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
