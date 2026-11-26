// Bounds on the cached wallet-info family (`info:`, `infoid:`).
//
// Rust unit tests cover the eviction and expiry logic against `MockStorage`.
// These two scenarios exercise the piece those cannot: `Storage::list_page`
// against real Durable Object storage, and the alarm-driven sweep that keeps
// the cache from growing without anyone reading it.
//
// Each scenario runs its own relay with a private `--persist-to` directory, so
// leftover cache entries from other tests or earlier runs cannot decide what
// is "oldest".

import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";
import { withRelay, connect, wait } from "./relay-harness.js";

const now = () => Math.floor(Date.now() / 1000);

function check(condition, message) {
  if (!condition) throw new Error(message);
}

/** Publishes a kind 13194 event and resolves once the relay accepted it. */
function publishInfo(ws, sk, createdAt) {
  const event = finalizeEvent(
    { kind: 13194, created_at: createdAt, tags: [], content: "" },
    sk,
  );
  ws.send(JSON.stringify(["EVENT", event]));
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("no OK for info event")), 5000);
    const handler = (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] !== "OK" || msg[1] !== event.id) return;
      clearTimeout(timer);
      ws.off("message", handler);
      if (msg[2]) resolve(event);
      else reject(new Error(`info event rejected: ${msg[3]}`));
    };
    ws.on("message", handler);
  });
}

/** Event ids the relay replays for `authors` before EOSE. */
function fetchInfo(ws, subId, author) {
  return new Promise((resolve, reject) => {
    const ids = [];
    const timer = setTimeout(() => reject(new Error(`no EOSE for ${subId}`)), 5000);
    const handler = (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[1] !== subId) return;
      if (msg[0] === "EVENT") ids.push(msg[2].id);
      if (msg[0] === "EOSE") {
        clearTimeout(timer);
        ws.off("message", handler);
        resolve(ids);
      }
    };
    ws.on("message", handler);
    ws.send(JSON.stringify(["REQ", subId, { kinds: [13194], authors: [author] }]));
  });
}

async function withStateDir(name, fn) {
  const dir = await mkdtemp(path.join(tmpdir(), `mekhala-${name}-`));
  try {
    return await fn(dir);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

export async function testInfoCacheCap() {
  console.log("\n--- Testing the info-cache size cap ---");

  await withStateDir("info-cap", async (persistTo) => {
    const vars = { MAX_INFO_EVENTS: 2, INFO_TTL_SECONDS: 0 };

    await withRelay({ port: 8793, vars, persistTo }, async ({ wsURL }) => {
      const publisher = await connect(wsURL);
      const base = now();
      const wallets = [];
      for (let i = 0; i < 3; i++) {
        const sk = generateSecretKey();
        wallets.push({ sk, pk: getPublicKey(sk), createdAt: base - (3 - i) });
      }
      for (const wallet of wallets) {
        await publishInfo(publisher, wallet.sk, wallet.createdAt);
      }
      console.log("Published 3 info events with a cap of 2.");

      const reader = await connect(wsURL);
      check(
        (await fetchInfo(reader, "newest", wallets[2].pk)).length === 1,
        "the newest wallet must still be cached",
      );
      check(
        (await fetchInfo(reader, "middle", wallets[1].pk)).length === 1,
        "the second wallet must still be cached",
      );
      check(
        (await fetchInfo(reader, "oldest", wallets[0].pk)).length === 0,
        "the oldest wallet should have been evicted",
      );
      console.log("✅ Cap kept the two newest info events and dropped the oldest.");

      // Eviction must not leave the evicted event's aliases behind: a `#p`
      // subscriber would otherwise get nothing while the key lingers.
      check(
        (await fetchInfo(reader, "oldest-again", wallets[0].pk)).length === 0,
        "an evicted event must not come back on a second request",
      );
      console.log("✅ Evicted event stayed gone.");

      reader.close();
      publisher.close();
    });
  });
}

/// More events than one storage page (128), so the cap only holds if trimming
/// pages through real Durable Object storage. Pubkeys are random, so key order
/// has nothing to do with age.
export async function testInfoCacheCapAcrossPages() {
  console.log("\n--- Testing the info-cache cap across storage pages ---");

  await withStateDir("info-pages", async (persistTo) => {
    const vars = { MAX_INFO_EVENTS: 200, INFO_TTL_SECONDS: 0, MESSAGE_RATE_LIMIT_PER_MINUTE: 0 };

    await withRelay({ port: 8793, vars, persistTo }, async ({ wsURL }) => {
      const publisher = await connect(wsURL);
      const base = now() - 300;
      const wallets = [];
      for (let i = 0; i < 201; i++) {
        const sk = generateSecretKey();
        wallets.push({ sk, pk: getPublicKey(sk), createdAt: base + i });
      }
      for (const wallet of wallets) {
        await publishInfo(publisher, wallet.sk, wallet.createdAt);
      }
      console.log("Published 201 info events with a cap of 200.");

      // One over the cap trims back to 90% of it: the 21 oldest go.
      const reader = await connect(wsURL);
      for (const [i, cached] of [[0, false], [20, false], [21, true], [200, true]]) {
        const found = (await fetchInfo(reader, `w${i}`, wallets[i].pk)).length === 1;
        check(found === cached, `wallet ${i} should ${cached ? "stay cached" : "be evicted"}`);
      }
      console.log("✅ Cap evicted the oldest events across every page.");

      reader.close();
      publisher.close();
    });
  });
}

export async function testInfoCacheExpiry() {
  console.log("\n--- Testing the info-cache time to live ---");

  await withStateDir("info-ttl", async (persistTo) => {
    // The sweep is driven by the alarm, which only runs while the connection
    // sweep is enabled; the idle timeout stays generous so the sockets this
    // test holds open are not closed mid-check.
    const vars = {
      MAX_INFO_EVENTS: 0,
      INFO_TTL_SECONDS: 2,
      IDLE_TIMEOUT_SECONDS: 120,
      CONNECTION_SWEEP_SECONDS: 1,
    };

    await withRelay({ port: 8794, vars, persistTo }, async ({ wsURL, log }) => {
      const sk = generateSecretKey();
      const pk = getPublicKey(sk);
      const publisher = await connect(wsURL);
      await publishInfo(publisher, sk, now());

      const reader = await connect(wsURL);
      check(
        (await fetchInfo(reader, "fresh", pk)).length === 1,
        "a freshly published info event must be served",
      );
      console.log("✅ Fresh info event served.");

      await wait(4000);

      check(
        (await fetchInfo(reader, "expired", pk)).length === 0,
        "an info event past its lifetime must not be served",
      );
      console.log("✅ Expired info event withheld.");

      // The alarm sweep must also have dropped it on its own, without waiting
      // for a reader: that is the only thing keeping an unread cache bounded.
      check(
        log().includes("info cache trimmed"),
        "the alarm sweep should have trimmed the expired entry",
      );
      console.log("✅ Alarm sweep trimmed the expired entry.");

      reader.close();
      publisher.close();
    });
  });
}
