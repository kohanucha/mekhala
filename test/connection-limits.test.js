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
