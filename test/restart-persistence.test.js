// Restart-based persistence test: proves the Durable Object storage adapter
// (`CloudflareStorage`) keeps the cached wallet-info registry across a full
// `wrangler dev` process restart. Rust unit tests use an in-memory
// `MockStorage`; this is the only test that exercises the real workerd
// storage backend end to end.
//
// What survives a restart (and is asserted here):
//   - `info:<pubkey>`      → the cached kind 13194 event
//   - `info-p:<pubkey>`    → the NWA `#p` alias
//   - `infoid:<event id>`  → kind 5 deletion by `e` tag
//   - republishing reads the previous event from storage and drops stale aliases
//
// Connection-scoped routing (`conn:`/`pk:`/`eid:`/`etag:`) is NOT asserted:
// a process restart kills every WebSocket, and reconnect allocates a new
// connection ID, so old subscription state is unreachable by design. The test
// does assert that fresh subscriptions still index and route after the
// restart, next to the recovered state.
//
// Runs only locally: it spawns its own relay on port 8790 with a dedicated
// `--persist-to` directory so it cannot clobber the main `.wrangler` state.

import { spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { WebSocket } from "ws";
import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";

const PORT = 8790;
const HTTP_URL = `http://127.0.0.1:${PORT}/`;
const WS_URL = `ws://127.0.0.1:${PORT}/`;
const START_TIMEOUT_MS = 120_000;
const EVENT_TIMEOUT_MS = 10_000;

const now = () => Math.floor(Date.now() / 1000);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function openSocket() {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(WS_URL);
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

async function publish(ws, event) {
  ws.send(JSON.stringify(["EVENT", event]));
  const ok = await waitForMessage(
    ws,
    (msg) => msg[0] === "OK" && msg[1] === event.id,
    EVENT_TIMEOUT_MS,
    `OK for kind ${event.kind}`,
  );
  assert(ok[2] === true, `relay rejected kind ${event.kind}: ${ok[3]}`);
}

// Sends a REQ and returns the events replayed before EOSE.
async function query(ws, subId, filter) {
  const events = [];
  const done = waitForMessage(
    ws,
    (msg) => {
      if (msg[0] === "EVENT" && msg[1] === subId) events.push(msg[2]);
      return msg[0] === "EOSE" && msg[1] === subId;
    },
    EVENT_TIMEOUT_MS,
    `EOSE ${subId}`,
  );
  ws.send(JSON.stringify(["REQ", subId, filter]));
  await done;
  ws.send(JSON.stringify(["CLOSE", subId]));
  return events;
}

function startRelay(persistDir) {
  const projectRoot = new URL("..", import.meta.url).pathname.replace(/\/$/, "");
  const child = spawn(
    "npx",
    [
      "wrangler",
      "dev",
      "--port",
      String(PORT),
      "--ip",
      "127.0.0.1",
      "--persist-to",
      persistDir,
    ],
    {
      cwd: projectRoot,
      detached: true,
      stdio: ["ignore", "pipe", "pipe"],
    },
  );

  let output = "";
  const collect = (chunk) => {
    output += chunk.toString();
    // Keep the buffer bounded; only the tail matters for diagnostics.
    if (output.length > 20_000) output = output.slice(-20_000);
  };
  child.stdout.on("data", collect);
  child.stderr.on("data", collect);
  child.logTail = () => output;

  return child;
}

async function waitForRelay(child) {
  const deadline = Date.now() + START_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) {
      throw new Error(`relay exited early (${child.exitCode})\n${child.logTail()}`);
    }
    try {
      const response = await fetch(HTTP_URL);
      if (response.ok) return;
    } catch {
      // not listening yet
    }
    await sleep(500);
  }
  throw new Error(`relay did not start in ${START_TIMEOUT_MS} ms\n${child.logTail()}`);
}

async function stopRelay(child) {
  if (!child || child.exitCode !== null) return;
  const exited = new Promise((resolve) => child.once("exit", resolve));
  try {
    process.kill(-child.pid, "SIGTERM");
  } catch {
    // already gone
  }
  const graceful = await Promise.race([exited.then(() => true), sleep(5000).then(() => false)]);
  if (!graceful) {
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch {
      // already gone
    }
    await Promise.race([exited, sleep(5000)]);
  }

  // The port must be free before the next start binds it.
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    try {
      await fetch(HTTP_URL);
    } catch {
      return;
    }
    await sleep(300);
  }
  throw new Error("relay port did not close after shutdown");
}

export async function testRestartPersistence() {
  console.log("\n--- Persistence across a wrangler dev restart ---");

  const persistDir = await mkdtemp(path.join(tmpdir(), "mekhala-restart-"));
  let relay = null;
  console.log(`  state dir: ${persistDir}`);

  try {
    // ---- Phase 1: populate durable state ----
    relay = startRelay(persistDir);
    await waitForRelay(relay);

    const walletSk = generateSecretKey();
    const walletPk = getPublicKey(walletSk);
    const appPk = getPublicKey(generateSecretKey());

    const phase1 = await openSocket();
    const info1 = finalizeEvent(
      {
        kind: 13194,
        created_at: now(),
        tags: [
          ["encryption", "nip44_v2"],
          ["notifications", "payment_received"],
          ["p", appPk],
        ],
        content: "get_info get_balance",
      },
      walletSk,
    );
    await publish(phase1, info1);

    // Leave a connection-scoped subscription behind (the socket is killed,
    // not closed, so shutdown behaviour does not affect the assertions).
    phase1.send(
      JSON.stringify([
        "REQ",
        "wallet-requests",
        { kinds: [23194], authors: [appPk], "#p": [walletPk] },
      ]),
    );
    await waitForMessage(phase1, (m) => m[0] === "EOSE" && m[1] === "wallet-requests", EVENT_TIMEOUT_MS, "phase-1 EOSE");
    console.log("  ✅ Phase 1: info event cached and subscription persisted.");

    // ---- Restart against the same persistence directory ----
    await stopRelay(relay);
    console.log("  ↻ relay stopped; restarting with the same --persist-to dir...");
    relay = startRelay(persistDir);
    await waitForRelay(relay);

    const ws = await openSocket();
    try {
      // 1. `info:<pubkey>` survived: authors replay.
      const byAuthor = await query(ws, "after-authors", {
        kinds: [13194],
        authors: [walletPk],
      });
      assert(byAuthor.length === 1, `authors replay after restart: ${byAuthor.length} event(s)`);
      assert(byAuthor[0].id === info1.id, "authors replay returned a different event");
      console.log("  ✅ Cached info event survived the restart (authors filter).");

      // 2. `info-p:<pubkey>` survived: NWA #p-only replay.
      const byPTag = await query(ws, "after-ptag", {
        kinds: [13194],
        "#p": [appPk],
      });
      assert(byPTag.length === 1, `#p replay after restart: ${byPTag.length} event(s)`);
      assert(byPTag[0].id === info1.id, "#p replay returned a different event");
      console.log("  ✅ NWA #p alias survived the restart.");

      // 3. Republishing reads the previous event from storage and drops the
      //    alias it no longer carries.
      const app2Pk = getPublicKey(generateSecretKey());
      const info2 = finalizeEvent(
        {
          kind: 13194,
          created_at: now(),
          tags: [["encryption", "nip44_v2"], ["p", app2Pk]],
          content: "get_info list_transactions",
        },
        walletSk,
      );
      await publish(ws, info2);

      const latest = await query(ws, "after-republish-authors", {
        kinds: [13194],
        authors: [walletPk],
      });
      assert(
        latest.length === 1 && latest[0].id === info2.id,
        `republished info not served as latest: got ${latest.length} event(s) ` +
          `${latest.map((e) => `${e.id.slice(0, 8)}(created_at=${e.created_at})`).join(",")}; ` +
          `want ${info2.id.slice(0, 8)}(created_at=${info2.created_at})`,
      );

      const staleAlias = await query(ws, "after-republish-stale-ptag", {
        kinds: [13194],
        "#p": [appPk],
      });
      assert(staleAlias.length === 0, `stale alias still replayed ${staleAlias.length} event(s)`);

      const freshAlias = await query(ws, "after-republish-new-ptag", {
        kinds: [13194],
        "#p": [app2Pk],
      });
      assert(freshAlias.length === 1 && freshAlias[0].id === info2.id, "new alias not replayed");
      console.log("  ✅ Republish replaced the event and pruned the stale p-tag alias.");

      // 4. Kind 5 by e tag resolves `infoid:` from storage after the restart.
      const deletion = finalizeEvent(
        { kind: 5, created_at: now(), tags: [["e", info2.id]], content: "" },
        walletSk,
      );
      await publish(ws, deletion);

      const afterDelete = await query(ws, "after-delete-authors", {
        kinds: [13194],
        authors: [walletPk],
      });
      assert(afterDelete.length === 0, `info survived deletion: ${afterDelete.length} event(s)`);

      const afterDeleteAlias = await query(ws, "after-delete-ptag", {
        kinds: [13194],
        "#p": [app2Pk],
      });
      assert(afterDeleteAlias.length === 0, `alias survived deletion: ${afterDeleteAlias.length} event(s)`);
      console.log("  ✅ Delete by e tag survived the restart (infoid mapping + alias cleanup).");

      // 5. Fresh routing still works next to the recovered state.
      const liveAppSk = generateSecretKey();
      const liveAppPk = getPublicKey(liveAppSk);
      const delivery = new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error("fresh subscription got no event after restart")), EVENT_TIMEOUT_MS);
        const onMessage = (data) => {
          const msg = JSON.parse(data.toString());
          if (msg[0] === "EVENT" && msg[1] === "after-restart-fresh") {
            clearTimeout(timer);
            ws.off("message", onMessage);
            resolve(msg[2]);
          }
        };
        ws.on("message", onMessage);
      });
      ws.send(
        JSON.stringify([
          "REQ",
          "after-restart-fresh",
          { kinds: [23194], authors: [liveAppPk], "#p": [walletPk] },
        ]),
      );
      await waitForMessage(ws, (m) => m[0] === "EOSE" && m[1] === "after-restart-fresh", EVENT_TIMEOUT_MS, "fresh EOSE");
      const request = finalizeEvent(
        { kind: 23194, created_at: now(), tags: [["p", walletPk]], content: "get_balance" },
        liveAppSk,
      );
      await publish(ws, request);
      const delivered = await delivery;
      assert(delivered.id === request.id, "fresh subscription delivered the wrong event");
      console.log("  ✅ New subscriptions index and route after the restart.");
    } finally {
      ws.close();
      phase1.close();
    }

    console.log("✅ Restart persistence: all checks passed.");
  } finally {
    await stopRelay(relay);
    await rm(persistDir, { recursive: true, force: true });
  }
}
