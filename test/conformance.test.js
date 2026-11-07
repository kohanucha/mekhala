// Conformance tests against the real `@getalby/sdk` (the client library used
// by Alby Go / Alby Hub). The existing tests build NIP-01 frames by hand,
// which can silently diverge from what the SDK actually sends. These tests
// drive `NWCClient` (app), `NWCWalletService` (wallet) and `NWAClient`
// (discovery) end-to-end, so the relay is exercised with the exact filters,
// tags and payload sizes real clients use.
//
// No Lightning node is involved: every wallet handler below returns a canned
// NIP-47 response. Covers:
//   - info discovery (`{kinds:[13194], limit:1, authors:[wallet]}`)
//   - wallet filters with `authors` + `#p` together
//   - client filters with `authors` + `#e` together
//   - request tags `p` / `v` / `encryption`
//   - NIP-44 v2 and NIP-04 request/response encryption
//   - notifications on both kinds (23197 nip44, 23196 nip04)
//   - NIP-47 error responses (`Nip47WalletError`)
//   - a near-maximum NIP-44 payload (>64 KiB ciphertext, <96 KiB relay cap)
//   - NWA discovery (`{kinds:[13194], "#p":[app]}`), then `getInfo`

import { WebSocket } from "ws";
import { nip44 } from "nostr-tools";
import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";
import {
  NWAClient,
  NWCClient,
  NWCWalletService,
  NWCWalletServiceKeyPair,
  Nip47WalletError,
} from "@getalby/sdk/nwc";

import { RELAY_URL } from "./env.js";

const CALL_TIMEOUT_MS = 30_000;
const RELAY_URLS = [RELAY_URL];

const hexKey = (sk) => Buffer.from(sk).toString("hex");
const now = () => Math.floor(Date.now() / 1000);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function withTimeout(promise, ms, label) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`timeout: ${label}`)), ms);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

async function until(predicate, ms, label) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await sleep(50);
  }
  throw new Error(`timeout waiting for ${label}`);
}

// The SDK's `subscribe*` calls return as soon as REQ is sent, not after the
// relay registered it. Publishing from a *different* connection can therefore
// race ahead of the subscription. A round trip on the subscriber's own
// connection flushes ordering: the relay processed the REQ before this reply.
async function flushSubscription(client) {
  await withTimeout(client.getWalletServiceInfo(), CALL_TIMEOUT_MS, "subscription barrier");
}

// Let in-flight response publishes settle before tearing down the pool,
// otherwise the SDK logs a spurious "connection closed by us" retry.
async function closeWalletService(walletService) {
  await sleep(250);
  walletService.close();
}

function openSocket() {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(RELAY_URL);
    ws.on("open", () => resolve(ws));
    ws.on("error", reject);
  });
}

function waitForMessage(ws, predicate, ms, label) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      ws.off("message", onMessage);
      reject(new Error(`timeout waiting for ${label}`));
    }, ms);
    function onMessage(data) {
      let msg;
      try {
        msg = JSON.parse(data.toString());
      } catch {
        return;
      }
      if (predicate(msg)) {
        clearTimeout(timer);
        ws.off("message", onMessage);
        resolve(msg);
      }
    }
    ws.on("message", onMessage);
  });
}

async function publishEvent(ws, event) {
  ws.send(JSON.stringify(["EVENT", event]));
  const ok = await waitForMessage(
    ws,
    (msg) => msg[0] === "OK" && msg[1] === event.id,
    15_000,
    `OK for kind ${event.kind}`,
  );
  if (ok[2] !== true) {
    throw new Error(`relay rejected kind ${event.kind}: ${ok[3]}`);
  }
  return ok;
}

// Publishes a kind 13194 info event through a raw socket so tests can choose
// the exact tags (encryption type, p tag). The wallet service hardcodes
// `nip04 nip44_v2`, which cannot express "nip04 only".
async function publishInfoEvent(walletSk, tags, content) {
  const ws = await openSocket();
  try {
    const event = finalizeEvent(
      { kind: 13194, created_at: now(), tags, content },
      walletSk,
    );
    await publishEvent(ws, event);
    return event;
  } finally {
    ws.close();
  }
}

// Subscribes a raw observer socket and resolves once the relay sent EOSE.
async function observe(ws, subId, filter) {
  const events = [];
  ws.on("message", (data) => {
    let msg;
    try {
      msg = JSON.parse(data.toString());
    } catch {
      return;
    }
    if (msg[0] === "EVENT" && msg[1] === subId) events.push(msg[2]);
  });
  ws.send(JSON.stringify(["REQ", subId, filter]));
  await waitForMessage(
    ws,
    (msg) => msg[0] === "EOSE" && msg[1] === subId,
    15_000,
    `EOSE ${subId}`,
  );
  return events;
}

function createTransaction(overrides = {}) {
  return {
    type: "incoming",
    state: "settled",
    invoice: "lnbc100n1p_test_invoice",
    description: "conformance test transaction",
    description_hash: "",
    preimage: "11".repeat(32),
    payment_hash: "22".repeat(32),
    amount: 21_000,
    fees_paid: 0,
    settled_at: now(),
    created_at: now(),
    expires_at: now() + 3600,
    ...overrides,
  };
}

const getInfoResult = {
  alias: "conformance-wallet",
  color: "#3399ff",
  pubkey: "33".repeat(32),
  network: "bitcoin",
  block_height: 800_000,
  block_hash: "44".repeat(32),
  methods: ["get_info", "get_balance", "pay_invoice", "make_invoice", "lookup_invoice", "list_transactions", "sign_message"],
  notifications: ["payment_received", "payment_sent"],
  lud16: "conformance@example.com",
};

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function assertEqual(actual, expected, label) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${label}: expected ${e}, got ${a}`);
}

// ---------------------------------------------------------------------------
// 1. Info discovery with the SDK's exact filter (limit:1 + authors)
// ---------------------------------------------------------------------------
export async function testSdkInfoDiscovery() {
  console.log("\n--- SDK conformance: info discovery (getWalletServiceInfo) ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const walletService = new NWCWalletService({ relayUrls: RELAY_URLS });
  const client = new NWCClient({
    relayUrls: RELAY_URLS,
    secret: hexKey(generateSecretKey()),
    walletPubkey: walletPk,
  });

  try {
    await withTimeout(
      walletService.publishWalletServiceInfoEvent(
        hexKey(walletSk),
        ["get_info", "get_balance", "pay_invoice"],
        ["payment_received", "payment_sent"],
      ),
      CALL_TIMEOUT_MS,
      "publishWalletServiceInfoEvent",
    );

    const info = await withTimeout(
      client.getWalletServiceInfo(),
      CALL_TIMEOUT_MS,
      "getWalletServiceInfo",
    );

    assert(
      info.encryptions.includes("nip44_v2"),
      `expected nip44_v2 in encryptions, got ${JSON.stringify(info.encryptions)}`,
    );
    assert(
      info.encryptions.includes("nip04"),
      `expected nip04 in encryptions, got ${JSON.stringify(info.encryptions)}`,
    );
    for (const method of ["get_info", "get_balance", "pay_invoice"]) {
      assert(
        info.capabilities.includes(method),
        `expected ${method} in capabilities, got ${JSON.stringify(info.capabilities)}`,
      );
    }
    assert(
      info.notifications.includes("payment_received"),
      `expected payment_received in notifications, got ${JSON.stringify(info.notifications)}`,
    );
    console.log("✅ SDK: info discovery via {kinds, limit:1, authors} passed.");
  } finally {
    client.close();
    await closeWalletService(walletService);
  }
}

// ---------------------------------------------------------------------------
// 2. All core methods over NIP-44, with the SDK's authors/#p and authors/#e
// ---------------------------------------------------------------------------
export async function testSdkWalletMethods() {
  console.log("\n--- SDK conformance: wallet methods over NIP-44 ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const appSk = generateSecretKey();
  const appPk = getPublicKey(appSk);

  const walletService = new NWCWalletService({ relayUrls: RELAY_URLS });
  const client = new NWCClient({
    relayUrls: RELAY_URLS,
    secret: hexKey(appSk),
    walletPubkey: walletPk,
  });
  const seen = new Set();

  try {
    await withTimeout(
      walletService.publishWalletServiceInfoEvent(hexKey(walletSk), ["get_info"], []),
      CALL_TIMEOUT_MS,
      "publishWalletServiceInfoEvent",
    );

    const keypair = new NWCWalletServiceKeyPair(hexKey(walletSk), appPk);
    const unsubscribe = await withTimeout(
      walletService.subscribe(keypair, {
        getInfo: async () => {
          seen.add("get_info");
          return { result: getInfoResult, error: undefined };
        },
        getBalance: async () => {
          seen.add("get_balance");
          return { result: { balance: 123_456 }, error: undefined };
        },
        payInvoice: async () => {
          seen.add("pay_invoice");
          return { result: { preimage: "55".repeat(32), fees_paid: 12 }, error: undefined };
        },
        makeInvoice: async (request) => {
          seen.add("make_invoice");
          return {
            result: createTransaction({ amount: request.amount, type: "incoming" }),
            error: undefined,
          };
        },
        lookupInvoice: async () => {
          seen.add("lookup_invoice");
          return { result: createTransaction({ state: "settled" }), error: undefined };
        },
        listTransactions: async () => {
          seen.add("list_transactions");
          return {
            result: {
              transactions: [createTransaction(), createTransaction({ type: "outgoing" })],
              total_count: 2,
            },
            error: undefined,
          };
        },
        signMessage: async (request) => {
          seen.add("sign_message");
          return {
            result: { message: request.message, signature: "ab".repeat(64) },
            error: undefined,
          };
        },
      }),
      CALL_TIMEOUT_MS,
      "walletService.subscribe",
    );

    try {
      const info = await withTimeout(client.getInfo(), CALL_TIMEOUT_MS, "getInfo");
      assertEqual(info.alias, getInfoResult.alias, "getInfo.alias");

      const balance = await withTimeout(client.getBalance(), CALL_TIMEOUT_MS, "getBalance");
      assertEqual(balance, { balance: 123_456 }, "getBalance");

      const paid = await withTimeout(
        client.payInvoice({ invoice: "lnbc210n1p_test" }),
        CALL_TIMEOUT_MS,
        "payInvoice",
      );
      assertEqual(paid, { preimage: "55".repeat(32), fees_paid: 12 }, "payInvoice");

      const invoice = await withTimeout(
        client.makeInvoice({ amount: 21_000, description: "test" }),
        CALL_TIMEOUT_MS,
        "makeInvoice",
      );
      assert(invoice.invoice.startsWith("lnbc"), "makeInvoice: missing invoice");
      assertEqual(invoice.amount, 21_000, "makeInvoice.amount");

      const lookedUp = await withTimeout(
        client.lookupInvoice({ payment_hash: "22".repeat(32) }),
        CALL_TIMEOUT_MS,
        "lookupInvoice",
      );
      assertEqual(lookedUp.state, "settled", "lookupInvoice.state");

      const listed = await withTimeout(
        client.listTransactions({ limit: 10 }),
        CALL_TIMEOUT_MS,
        "listTransactions",
      );
      assertEqual(listed.transactions.length, 2, "listTransactions.length");
      assertEqual(listed.total_count, 2, "listTransactions.total_count");

      const signed = await withTimeout(
        client.signMessage({ message: "hello mekhala" }),
        CALL_TIMEOUT_MS,
        "signMessage",
      );
      assertEqual(signed.message, "hello mekhala", "signMessage.message");
      assertEqual(signed.signature, "ab".repeat(64), "signMessage.signature");

      // Every method must have reached the wallet through the SDK's
      // `{kinds:[23194], authors:[app], "#p":[wallet]}` filter.
      assertEqual(
        [...seen].sort(),
        [
          "get_balance",
          "get_info",
          "list_transactions",
          "lookup_invoice",
          "make_invoice",
          "pay_invoice",
          "sign_message",
        ],
        "methods handled by the wallet",
      );
      console.log("✅ SDK: 7 methods over NIP-44 (authors+#p / authors+#e) passed.");
    } finally {
      unsubscribe();
    }
  } finally {
    client.close();
    await closeWalletService(walletService);
  }
}

// ---------------------------------------------------------------------------
// 2b. Exact request event shape the SDK publishes (p / v / encryption tags)
// and the SDK's response filter ({kinds, authors, "#e"}) on a real reply.
// ---------------------------------------------------------------------------
export async function testSdkRequestShape() {
  console.log("\n--- SDK conformance: request shape and response routing ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const appSk = generateSecretKey();
  const appPk = getPublicKey(appSk);

  await publishInfoEvent(
    walletSk,
    [["encryption", "nip44_v2"]],
    "get_balance",
  );

  const client = new NWCClient({
    relayUrls: RELAY_URLS,
    secret: hexKey(appSk),
    walletPubkey: walletPk,
  });
  // The raw socket plays the wallet so the test can inspect the request
  // without competing with NWCWalletService for last-in-wins delivery.
  const walletSocket = await openSocket();

  try {
    const requests = await observe(walletSocket, "shape-requests", {
      kinds: [23194],
      authors: [appPk],
      "#p": [walletPk],
    });

    const balancePromise = client.getBalance();
    await until(() => requests.length === 1, 10_000, "SDK request event");

    const request = requests[0];
    const tag = (name) => request.tags.find((t) => t[0] === name)?.[1];
    assertEqual(tag("p"), walletPk, "request p tag");
    assertEqual(tag("v"), "1.0", "request v tag");
    assertEqual(tag("encryption"), "nip44_v2", "request encryption tag");

    const conversationKey = nip44.v2.utils.getConversationKey(walletSk, appPk);
    const replyContent = nip44.v2.encrypt(
      JSON.stringify({ result_type: "get_balance", result: { balance: 4242 } }),
      conversationKey,
    );
    const reply = finalizeEvent(
      {
        kind: 23195,
        created_at: now(),
        tags: [
          ["e", request.id],
          ["p", appPk],
        ],
        content: replyContent,
      },
      walletSk,
    );
    await publishEvent(walletSocket, reply);

    const balance = await withTimeout(balancePromise, CALL_TIMEOUT_MS, "getBalance reply");
    assertEqual(balance, { balance: 4242 }, "SDK-decrypted balance");
    console.log("✅ SDK: real request tags accepted; {authors, #e} reply routed.");
  } finally {
    walletSocket.close();
    client.close();
  }
}

// ---------------------------------------------------------------------------
// 3. NIP-04 compatibility (kind 23196 notifications, nip04 encryption tag)
// ---------------------------------------------------------------------------
export async function testSdkNip04Compatibility() {
  console.log("\n--- SDK conformance: NIP-04 (nip04-only wallet) ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const appSk = generateSecretKey();
  const appPk = getPublicKey(appSk);

  // NIP-04-only info event; the SDK's _selectEncryptionType must pick nip04.
  await publishInfoEvent(
    walletSk,
    [["encryption", "nip04"], ["notifications", "payment_received"]],
    "get_balance pay_invoice",
  );

  const walletService = new NWCWalletService({ relayUrls: RELAY_URLS });
  const client = new NWCClient({
    relayUrls: RELAY_URLS,
    secret: hexKey(appSk),
    walletPubkey: walletPk,
  });

  try {
    const keypair = new NWCWalletServiceKeyPair(hexKey(walletSk), appPk);
    const unsubscribe = await withTimeout(
      walletService.subscribe(keypair, {
        getBalance: async () => ({ result: { balance: 777 }, error: undefined }),
        payInvoice: async () => ({
          result: { preimage: "66".repeat(32), fees_paid: 1 },
          error: undefined,
        }),
      }),
      CALL_TIMEOUT_MS,
      "walletService.subscribe (nip04)",
    );

    try {
      const balance = await withTimeout(client.getBalance(), CALL_TIMEOUT_MS, "getBalance (nip04)");
      assertEqual(balance, { balance: 777 }, "getBalance over nip04");
      assertEqual(client.encryptionType, "nip04", "selected encryption");

      // Notifications: the SDK subscribes to kind 23196 when encryption is
      // nip04 and the wallet publishes both 23196 and 23197.
      const received = [];
      const unsubscribeNotifications = await withTimeout(
        client.subscribeNotifications((notification) => received.push(notification)),
        CALL_TIMEOUT_MS,
        "subscribeNotifications (nip04)",
      );
      await flushSubscription(client);
      try {
        await withTimeout(
          walletService.publishNotification(keypair, {
            notification_type: "payment_received",
            notification: createTransaction({ type: "incoming" }),
          }),
          CALL_TIMEOUT_MS,
          "publishNotification (nip04)",
        );
        await until(() => received.length >= 1, 10_000, "nip04 notification");
        assertEqual(received[0].notification_type, "payment_received", "notification type");
      } finally {
        unsubscribeNotifications();
      }
      console.log("✅ SDK: nip04 request/response + kind 23196 notification passed.");
    } finally {
      unsubscribe();
    }
  } finally {
    client.close();
    await closeWalletService(walletService);
  }
}

// ---------------------------------------------------------------------------
// 4. NIP-44 notifications (kind 23197) and the parallel nip04 publish
// ---------------------------------------------------------------------------
export async function testSdkNotifications() {
  console.log("\n--- SDK conformance: NIP-44 notification (kind 23197) ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const appSk = generateSecretKey();
  const appPk = getPublicKey(appSk);

  const walletService = new NWCWalletService({ relayUrls: RELAY_URLS });
  const client = new NWCClient({
    relayUrls: RELAY_URLS,
    secret: hexKey(appSk),
    walletPubkey: walletPk,
  });
  const nip04Observer = await openSocket();

  try {
    await withTimeout(
      walletService.publishWalletServiceInfoEvent(
        hexKey(walletSk),
        ["get_info"],
        ["payment_received", "payment_sent"],
      ),
      CALL_TIMEOUT_MS,
      "publishWalletServiceInfoEvent",
    );

    // The wallet publishes both kinds; observe the nip04 copy separately to
    // prove the relay routes 23196 (no existing test covers it).
    const nip04Notifications = await observe(nip04Observer, "watch-23196", {
      kinds: [23196],
      authors: [walletPk],
      "#p": [appPk],
    });

    const keypair = new NWCWalletServiceKeyPair(hexKey(walletSk), appPk);
    const received = [];
    const unsubscribeNotifications = await withTimeout(
      client.subscribeNotifications((notification) => received.push(notification)),
      CALL_TIMEOUT_MS,
      "subscribeNotifications",
    );
    await flushSubscription(client);

    try {
      await withTimeout(
        walletService.publishNotification(keypair, {
          notification_type: "payment_sent",
          notification: createTransaction({ type: "outgoing" }),
        }),
        CALL_TIMEOUT_MS,
        "publishNotification",
      );

      await until(() => received.length >= 1, 10_000, "nip44 notification");
      assertEqual(received[0].notification_type, "payment_sent", "notification type");

      await until(() => nip04Notifications.length >= 1, 10_000, "kind 23196 routing");
      assertEqual(nip04Notifications.length, 1, "23196 events routed");
      console.log("✅ SDK: kind 23197 delivered to client, kind 23196 routed.");
    } finally {
      unsubscribeNotifications();
    }
  } finally {
    nip04Observer.close();
    client.close();
    await closeWalletService(walletService);
  }
}

// ---------------------------------------------------------------------------
// 5. NIP-47 error response path
// ---------------------------------------------------------------------------
export async function testSdkWalletError() {
  console.log("\n--- SDK conformance: wallet error response (Nip47WalletError) ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const appSk = generateSecretKey();
  const appPk = getPublicKey(appSk);

  const walletService = new NWCWalletService({ relayUrls: RELAY_URLS });
  const client = new NWCClient({
    relayUrls: RELAY_URLS,
    secret: hexKey(appSk),
    walletPubkey: walletPk,
  });

  try {
    await withTimeout(
      walletService.publishWalletServiceInfoEvent(hexKey(walletSk), ["pay_invoice"], []),
      CALL_TIMEOUT_MS,
      "publishWalletServiceInfoEvent",
    );

    const keypair = new NWCWalletServiceKeyPair(hexKey(walletSk), appPk);
    const unsubscribe = await withTimeout(
      walletService.subscribe(keypair, {
        payInvoice: async () => ({
          result: undefined,
          error: { code: "INSUFFICIENT_BALANCE", message: "not enough sats" },
        }),
      }),
      CALL_TIMEOUT_MS,
      "walletService.subscribe",
    );

    try {
      let error;
      try {
        await withTimeout(
          client.payInvoice({ invoice: "lnbc1broken" }),
          CALL_TIMEOUT_MS,
          "payInvoice (expected error)",
        );
      } catch (err) {
        error = err;
      }
      assert(error, "payInvoice should have rejected");
      assert(
        error instanceof Nip47WalletError,
        `expected Nip47WalletError, got ${error?.constructor?.name}: ${error?.message}`,
      );
      assertEqual(error.code, "INSUFFICIENT_BALANCE", "error code");
      console.log("✅ SDK: error response surfaced as Nip47WalletError.");
    } finally {
      unsubscribe();
    }
  } finally {
    client.close();
    await closeWalletService(walletService);
  }
}

// ---------------------------------------------------------------------------
// 6. Near-maximum NIP-44 payload (>64 KiB ciphertext, <96 KiB relay cap)
// ---------------------------------------------------------------------------
export async function testSdkLargeResponse() {
  console.log("\n--- SDK conformance: near-maximum NIP-44 response payload ---");

  // NIP-44 v2 rejects plaintext > 65,535 bytes; base64 of that ceiling is
  // ~87 KiB, which only fits because MAX_CONTENT_LENGTH is 98,304. Keeping
  // the plaintext under 65,535 while pushing the ciphertext over 65,536
  // guards the raised limit against a regression back to 64 KiB.
  const MAX_PLAINTEXT = 65_535;
  const TARGET_PLAINTEXT = 64_000;
  const transactions = [];
  let plaintextSize = 0;
  for (let i = 0; i < 512; i += 1) {
    const candidate = createTransaction({
      invoice: `lnbc${i}n1p_${"9".repeat(80)}`,
      description: `tx ${i} ${"d".repeat(220)}`,
      payment_hash: (i % 100).toString(16).padStart(2, "0").repeat(32),
    });
    const next = [...transactions, candidate];
    const size = Buffer.byteLength(
      JSON.stringify({
        result_type: "list_transactions",
        result: { transactions: next, total_count: next.length },
      }),
    );
    if (size > TARGET_PLAINTEXT) break;
    transactions.push(candidate);
    plaintextSize = size;
  }
  assert(
    plaintextSize > 60_000 && plaintextSize <= MAX_PLAINTEXT,
    `bad fixture size: ${plaintextSize}`,
  );

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const appSk = generateSecretKey();
  const appPk = getPublicKey(appSk);

  const walletService = new NWCWalletService({ relayUrls: RELAY_URLS });
  const client = new NWCClient({
    relayUrls: RELAY_URLS,
    secret: hexKey(appSk),
    walletPubkey: walletPk,
  });

  try {
    await withTimeout(
      walletService.publishWalletServiceInfoEvent(hexKey(walletSk), ["list_transactions"], []),
      CALL_TIMEOUT_MS,
      "publishWalletServiceInfoEvent",
    );

    const keypair = new NWCWalletServiceKeyPair(hexKey(walletSk), appPk);
    const unsubscribe = await withTimeout(
      walletService.subscribe(keypair, {
        listTransactions: async () => ({
          result: { transactions, total_count: transactions.length },
          error: undefined,
        }),
      }),
      CALL_TIMEOUT_MS,
      "walletService.subscribe",
    );

    try {
      const listed = await withTimeout(
        client.listTransactions({ limit: 500 }),
        CALL_TIMEOUT_MS,
        "listTransactions (large)",
      );
      assertEqual(listed.transactions.length, transactions.length, "large response length");

      // NIP-44 padding is deterministic, so the exact ciphertext size the
      // wallet published can be reproduced locally. The successful round trip
      // above proves the relay accepted that same payload.
      const plaintext = JSON.stringify({
        result_type: "list_transactions",
        result: { transactions, total_count: transactions.length },
      });
      const ciphertextLength = nip44.v2.encrypt(
        plaintext,
        nip44.v2.utils.getConversationKey(walletSk, appPk),
      ).length;
      assert(
        ciphertextLength > 65_536,
        `expected ciphertext > 65,536 bytes, got ${ciphertextLength}`,
      );
      assert(
        ciphertextLength < 98_304,
        `expected ciphertext < 98,304 bytes, got ${ciphertextLength}`,
      );
      console.log(
        `✅ SDK: ${plaintextSize}-byte plaintext → ${ciphertextLength}-byte ciphertext accepted (${transactions.length} transactions).`,
      );
    } finally {
      unsubscribe();
    }
  } finally {
    client.close();
    await closeWalletService(walletService);
  }
}

// ---------------------------------------------------------------------------
// 7. NWA discovery (#p-only info replay) then getInfo through the new client
// ---------------------------------------------------------------------------
export async function testSdkNwaDiscovery() {
  console.log("\n--- SDK conformance: NWA discovery + getInfo ---");

  const walletSk = generateSecretKey();
  const nwaSk = generateSecretKey();
  const nwaPk = getPublicKey(nwaSk);

  const walletService = new NWCWalletService({ relayUrls: RELAY_URLS });
  const nwa = new NWAClient({
    relayUrls: RELAY_URLS,
    appSecretKey: hexKey(nwaSk),
    requestMethods: ["get_info", "get_balance"],
    name: "Conformance App",
  });

  try {
    // A wallet answering an NWA request publishes its info event addressed to
    // the app through a p tag (the NWA discovery pattern).
    await publishInfoEvent(
      walletSk,
      [
        ["encryption", "nip44_v2"],
        ["notifications", "payment_received"],
        ["p", nwaPk],
      ],
      "get_info get_balance",
    );

    const keypair = new NWCWalletServiceKeyPair(hexKey(walletSk), nwaPk);
    const unsubscribe = await withTimeout(
      walletService.subscribe(keypair, {
        getInfo: async () => ({ result: getInfoResult, error: undefined }),
      }),
      CALL_TIMEOUT_MS,
      "walletService.subscribe (NWA)",
    );

    try {
      const client = await withTimeout(
        new Promise((resolve, reject) => {
          nwa
            .subscribe({
              onSuccess: (nwcClient) => {
                try {
                  nwcClient.getInfo().then(resolve).catch(reject);
                } catch (err) {
                  reject(err);
                }
              },
            })
            .catch(reject);
        }),
        CALL_TIMEOUT_MS,
        "NWA subscribe + getInfo",
      );

      assertEqual(client.alias, getInfoResult.alias, "NWA getInfo.alias");
      assertEqual(getPublicKey(nwaSk), nwaPk, "NWA app key");
      console.log("✅ SDK: NWA #p-only info replay + getInfo passed.");
    } finally {
      unsubscribe();
    }
  } finally {
    nwa.close();
    await closeWalletService(walletService);
  }
}

export async function runSdkConformance() {
  await testSdkInfoDiscovery();
  await testSdkWalletMethods();
  await testSdkRequestShape();
  await testSdkNip04Compatibility();
  await testSdkNotifications();
  await testSdkWalletError();
  await testSdkLargeResponse();
  await testSdkNwaDiscovery();
}
