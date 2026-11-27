import { WebSocket } from "ws";
import { withRelay, connect, closedWithin, refused, wait } from "./relay-harness.js";

/** A subscription filter the relay accepts (it needs `authors` or a tag). */
const WALLET_FILTER = { kinds: [13194], authors: ["0".repeat(64)] };

function check(condition, message) {
  if (!condition) throw new Error(message);
}

/** Resolves when the relay answers `subId` with EOSE. */
function waitForEose(ws, subId, ms = 5000) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`no EOSE for ${subId}`)), ms);
    const handler = (data) => {
      const message = JSON.parse(data.toString());
      if (message[0] === "EOSE" && message[1] === subId) {
        clearTimeout(timer);
        ws.off("message", handler);
        resolve();
      }
    };
    ws.on("message", handler);
  });
}

export async function testPerAddressLimits() {
  console.log("\n--- Testing per-address and per-network connection limits ---");

  const vars = { MAX_CONNECTIONS_PER_IP: 2, MAX_CONNECTIONS_PER_ASN: 3 };

  await withRelay({ port: 8789, vars }, async ({ wsURL }) => {
    const addressA = { "CF-Connecting-IP": "203.0.113.9" };
    const addressB = { "CF-Connecting-IP": "203.0.113.10" };
    const addressC = { "CF-Connecting-IP": "203.0.113.11" };

    const first = await connect(wsURL, addressA);
    const second = await connect(wsURL, addressA);
    check(
      await refused(wsURL, addressA),
      "a third connection from one address should have been refused",
    );
    console.log("✅ Address limit refused the third connection from the same address.");

    // A different address still fits, but it shares the network with A: the
    // network limit is what stops the next one.
    const third = await connect(wsURL, addressB);
    check(
      await refused(wsURL, addressC),
      "a fourth address in one network should have been refused",
    );
    console.log("✅ Network limit refused another address in the same network.");

    // The caps count live connections, they are not a permanent ban.
    for (const ws of [first, second, third]) ws.close();
    await wait(500);
    const again = await connect(wsURL, addressC);
    check(
      again.readyState === WebSocket.OPEN,
      "a client should be admitted again once its connections are gone",
    );
    again.close();
    console.log("✅ Limits release when the connections close.");
  });
}

export async function testIdleConnectionSweep() {
  console.log("\n--- Testing the idle connection sweep ---");

  const vars = { IDLE_TIMEOUT_SECONDS: 2, CONNECTION_SWEEP_SECONDS: 1 };

  await withRelay({ port: 8791, vars }, async ({ wsURL }) => {
    const idle = await connect(wsURL);
    check(
      await closedWithin(idle, 8000),
      "an idle connection with no subscription should have been closed",
    );
    console.log("✅ Idle connection closed.");

    // The alarm is one-shot: a second idle socket only closes if the sweep
    // re-armed itself.
    const idleAgain = await connect(wsURL);
    check(
      await closedWithin(idleAgain, 8000),
      "the sweep should have kept running after it fired",
    );
    console.log("✅ Sweep re-armed itself.");

    const wallet = await connect(wsURL);
    wallet.send(JSON.stringify(["REQ", "idle-wallet", WALLET_FILTER]));
    await waitForEose(wallet, "idle-wallet");
    await wait(4000);
    check(
      wallet.readyState === WebSocket.OPEN,
      "a connection holding a subscription must not be swept",
    );
    wallet.close();
    console.log("✅ A connection with a subscription survives the sweep.");
  });
}

export async function testConnectionEviction() {
  console.log("\n--- Testing connection slot reclaiming ---");

  const vars = { MAX_CONNECTIONS: 2, EVICT_MIN_IDLE_SECONDS: 2 };

  await withRelay({ port: 8792, vars }, async ({ wsURL }) => {
    const wallet = await connect(wsURL);
    wallet.send(JSON.stringify(["REQ", "evict-wallet", WALLET_FILTER]));
    await waitForEose(wallet, "evict-wallet");

    const idle = await connect(wsURL);
    await wait(3500);

    // The relay is full; reclaiming an idle slot is what lets a real client
    // in instead of turning it away.
    const newcomer = await connect(wsURL);
    check(
      newcomer.readyState === WebSocket.OPEN,
      "a full relay should reclaim an idle slot instead of refusing",
    );
    check(
      await closedWithin(idle, 3000),
      "the idle connection should be the one that was closed",
    );
    check(
      wallet.readyState === WebSocket.OPEN,
      "a connection holding a subscription must never be evicted",
    );
    console.log("✅ Full relay reclaimed an idle slot and kept the subscribed wallet.");

    newcomer.close();
    wallet.close();
  });
}

export async function testMessageBudget() {
  console.log("\n--- Testing the per-connection message budget ---");

  // Small enough to spend in a few frames, large enough to be legal traffic.
  const vars = { MESSAGE_RATE_LIMIT_PER_MINUTE: 6 };

  await withRelay({ port: 8802, vars }, async ({ wsURL }) => {
    const client = await connect(wsURL);
    const notices = [];
    client.on("message", (data) => {
      const message = JSON.parse(data.toString());
      if (message[0] === "NOTICE") notices.push(message[1]);
    });

    // A `CLOSE` for a subscription that was never opened is the cheapest
    // message a client can send, and it used to rewrite the whole connection
    // snapshot. The budget has to count it all the same.
    for (let i = 0; i < 6; i += 1) {
      client.send(JSON.stringify(["CLOSE", `missing-${i}`]));
    }
    await wait(500);

    client.send(JSON.stringify(["CLOSE", "one-too-many"]));
    check(
      await closedWithin(client, 5000),
      "a connection over its message budget should have been closed",
    );
    check(
      notices.some((notice) => notice.includes("rate limited")),
      `the relay should say why it closed, got ${JSON.stringify(notices)}`,
    );
    console.log("✅ The message budget closed a client that kept sending.");

    // The budget is per connection, so a reconnect does not inherit it. The
    // connection caps are what bound how often a client can start over.
    const fresh = await connect(wsURL);
    fresh.send(JSON.stringify(["CLOSE", "fresh"]));
    await wait(500);
    check(
      fresh.readyState === WebSocket.OPEN,
      "a new connection must start with a fresh message budget",
    );
    fresh.close();
    console.log("✅ A new connection starts with a fresh budget.");
  });
}

/**
 * A deploy drops every socket without a close event, so the relay never
 * cleans those connections up on its own. Killing the relay with sockets open
 * and restarting it on the same storage reproduces that.
 *
 * Two clean-ups are checked separately, so they cannot race for the same
 * connection: an event for a dead subscriber removes it at once, and the
 * alarm removes the one no event ever reaches.
 */
export async function testOrphanedConnectionCleanup() {
  console.log("\n--- Testing clean-up of connections lost without a close ---");
  const { mkdtemp, rm } = await import("node:fs/promises");
  const { tmpdir } = await import("node:os");
  const path = await import("node:path");
  const { finalizeEvent, generateSecretKey, getPublicKey } = await import("nostr-tools/pure");

  const persistTo = await mkdtemp(path.join(tmpdir(), "mekhala-orphans-"));
  const walletSk = generateSecretKey();
  const wallet = getPublicKey(walletSk);
  const port = 8803;

  try {
    await withRelay({ port, persistTo }, async ({ wsURL }) => {
      const app = await connect(wsURL);
      app.send(JSON.stringify(["REQ", "replies", { kinds: [23195], authors: [wallet] }]));
      await waitForEose(app, "replies");
      const idle = await connect(wsURL);
      idle.send(JSON.stringify(["REQ", "cold", { kinds: [23195], authors: ["f".repeat(64)] }]));
      await waitForEose(idle, "cold");
      console.log("Two subscribers saved; stopping the relay with their sockets open.");
    });

    await withRelay({ port, persistTo, vars: { ORPHAN_SWEEP_SECONDS: 0 } }, async ({ wsURL, log }) => {
      const ws = await connect(wsURL);
      for (let i = 0; i < 3; i++) {
        const reply = finalizeEvent({
          kind: 23195,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", getPublicKey(generateSecretKey())], ["e", "a".repeat(64)]],
          content: "reply",
        }, walletSk);
        ws.send(JSON.stringify(["EVENT", reply]));
        await new Promise((resolve) => {
          const handler = (data) => {
            const msg = JSON.parse(data.toString());
            if (msg[0] === "OK" && msg[1] === reply.id) {
              ws.off("message", handler);
              resolve();
            }
          };
          ws.on("message", handler);
        });
      }
      await wait(300);
      const removed = log().split("\n").filter((line) => line.includes("has no socket: removing its state"));
      check(removed.length === 1, `expected one immediate clean-up, saw ${removed.length}`);
      check(!log().includes("send failed"), "an event was sent to a connection that no longer exists");
      console.log("✅ The first event for a dead subscriber removed it; nothing was sent to it.");
      ws.close();
    });

    const vars = { IDLE_TIMEOUT_SECONDS: 120, CONNECTION_SWEEP_SECONDS: 1, ORPHAN_SWEEP_SECONDS: 1 };
    await withRelay({ port, persistTo, vars }, async ({ wsURL, log }) => {
      const ws = await connect(wsURL); // arms the alarm
      const deadline = Date.now() + 8000;
      while (!log().includes("orphan sweep: removed state of 1 connection(s)") && Date.now() < deadline) {
        await wait(250);
      }
      check(
        log().includes("orphan sweep: removed state of 1 connection(s)"),
        "the alarm did not remove the subscriber no event reaches",
      );
      console.log("✅ The alarm removed the dead subscriber no event reaches.");
      ws.close();
    });
  } finally {
    await rm(persistTo, { recursive: true, force: true });
  }
}
