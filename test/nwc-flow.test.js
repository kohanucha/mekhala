import { WebSocket } from "ws";
import { RELAY_URL, HTTP_URL } from "./env.js";
import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";

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

export async function testRelay() {
  const ws = new WebSocket(RELAY_URL);

  const sk = generateSecretKey();
  const pk = getPublicKey(sk);
  let eventKind3;

  return new Promise((resolve, reject) => {
    ws.on("open", async () => {
      console.log("Connected to relay.");

      console.log("Testing Stateless REQ...");
      ws.send(JSON.stringify(["REQ", "sub-1", { kinds: [23194], "#p": [pk] }]));

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

      console.log("Testing Restricted Kinds (Kind 1)...");
      eventKind3 = finalizeEvent(
        {
          kind: 1,
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

      if (eoseReceived && sigRejected && kind3Rejected && !routingTested) {
        routingTested = true;
        testRouting(ws, sk, pk).then(resolve).catch(reject);
      }
    });

    ws.on("error", reject);
    setTimeout(() => {
      if (!routingTested) reject(new Error("Timeout"));
    }, 5000);
  });
}

export async function testNwcFlow() {
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

export async function testStrictValidation() {
  console.log("\n--- Testing Strict NIP-47 Validation ---");
  const ws = new WebSocket(RELAY_URL);

  return new Promise((resolve, reject) => {
    const sk = generateSecretKey();
    const pk = getPublicKey(sk);

    ws.on("open", () => {
      console.log("Step 1: Testing rejection of broad NIP-47 REQ...");
      ws.send(JSON.stringify(["REQ", "broad-sub", { kinds: [23194] }]));

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

export async function testMultiClientIsolation() {
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

export async function testEdgeCases() {
  console.log("\n--- Testing NWC Edge Cases (Performance & Protocol) ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  await new Promise((resolve, reject) => {
    ws.on("open", async () => {
      console.log("Step 1: Testing reference counting with multiple subscriptions...");
      ws.send(JSON.stringify(["REQ", "sub-a", { kinds: [23194], "#p": [pk] }]));
      ws.send(JSON.stringify(["REQ", "sub-b", { kinds: [23194], "#p": [pk] }]));

      console.log("Step 2: Testing signature verification cache...");
      const event = finalizeEvent({
        kind: 23194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [["p", pk]],
        content: "cache-test"
      }, sk);

      ws.send(JSON.stringify(["EVENT", event]));
      ws.send(JSON.stringify(["EVENT", event]));

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

export async function testLastInWinsRouting() {
  console.log("\n--- Testing Last-In-Wins Routing (Singular Routing) ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const appSk = generateSecretKey();

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

export async function testMaxConnections() {
  console.log("\n--- Testing Max Connections (429 Too Many Requests) ---");

  const LIMIT_PORT = 8788;
  const limitSecret = "limit-test-secret";
  const limitWsURL = `ws://localhost:${LIMIT_PORT}/${limitSecret}`;
  const limitHttpURL = `http://localhost:${LIMIT_PORT}/${limitSecret}`;

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

  let serverReady = false;
  for (let i = 0; i < 30; i++) {
    await new Promise(r => setTimeout(r, 2000));
    try {
      const resp = await fetch(limitHttpURL);
      if (resp.status === 404 || resp.status === 200) {
        serverReady = true;
        break;
      }
    } catch (e) {}
  }

  if (!serverReady) {
    cleanup();
    throw new Error("Limited relay failed to start within 60 seconds");
  }
  console.log("Limited relay is up.");

  try {
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

    for (const ws of connections) {
      ws.close();
    }
  } finally {
    cleanup();
  }
}
