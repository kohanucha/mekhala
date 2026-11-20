// The admin channel: who may change Lightning Address registrations.
//
// The Rust tests cover the guard on its own, but the parts that matter
// operationally only exist end-to-end: a deployment with no admin credential
// must not reveal the channel, the relay password must not open it, a wallet
// that does not grant `pay_invoice` must be refused before anything is
// written, and a registration must survive a sealing-key rotation.

import { WebSocket } from "ws";
import { nip04 } from "nostr-tools";
import { finalizeEvent, getPublicKey } from "nostr-tools/pure";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { withRelay, TEST_LN_ADDRESS_KEY } from "./relay-harness.js";

const PORT = 8801;
const ADMIN_SECRET = "admin-test-secret";
const ROTATED_LN_ADDRESS_KEY = "c3".repeat(32);

function check(condition, message) {
  if (!condition) throw new Error(message);
}

function now() {
  return Math.floor(Date.now() / 1000);
}

function walletSecret(byte) {
  return new Uint8Array(32).fill(byte);
}

function walletPubkey(byte) {
  return getPublicKey(walletSecret(byte));
}

/** A connection string for a mock wallet that never spends anything. */
function connectionUri(byte, relaySecret) {
  const pk = walletPubkey(byte);
  const nwcSecret = Buffer.alloc(32, byte).toString("hex");
  const relay = encodeURIComponent(`ws://127.0.0.1:${PORT}/${relaySecret}`);
  return `nostr+walletconnect://${pk}?relay=${relay}&secret=${nwcSecret}`;
}

function adminUrl(suffix) {
  return `http://127.0.0.1:${PORT}/admin${suffix}`;
}

async function adminFetch(method, suffix, { secret = ADMIN_SECRET, body } = {}) {
  const headers = {};
  if (secret !== null) headers.Authorization = `Bearer ${secret}`;
  const init = { method, headers };
  if (body !== undefined) {
    headers["Content-Type"] = "application/json";
    init.body = JSON.stringify(body);
  }
  return fetch(adminUrl(suffix), init);
}

async function wellKnown(username) {
  const response = await fetch(
    `http://127.0.0.1:${PORT}/.well-known/lnurlp/${username}`,
  );
  return { status: response.status, body: await response.json() };
}

/**
 * A wallet that announces itself, subscribes to its own requests, and answers
 * `get_info` with `methods`. Returns once the relay has acknowledged the
 * subscription, so a registration that follows finds the wallet reachable.
 */
function mockWallet({ relaySecret, secretByte, methods }) {
  const sk = walletSecret(secretByte);
  const pk = getPublicKey(sk);
  const ws = new WebSocket(`ws://127.0.0.1:${PORT}/${relaySecret}`);

  const ready = new Promise((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error("mock wallet did not become reachable")),
      8000,
    );

    ws.on("open", () => {
      const info = finalizeEvent(
        {
          kind: 13194,
          created_at: now(),
          tags: [],
          content: methods.join(" "),
        },
        sk,
      );
      ws.send(JSON.stringify(["EVENT", info]));
      ws.send(
        JSON.stringify(["REQ", "wallet-admin-sub", { kinds: [23194], "#p": [pk] }]),
      );
    });

    ws.on("message", async (raw) => {
      const message = JSON.parse(raw.toString());

      if (message[0] === "EOSE") {
        clearTimeout(timer);
        resolve({ ws, pk });
        return;
      }
      if (message[0] !== "EVENT" || message[1] !== "wallet-admin-sub") return;

      const event = message[2];
      try {
        const request = JSON.parse(
          await nip04.decrypt(sk, event.pubkey, event.content),
        );
        const result =
          request.method === "get_info"
            ? { alias: "mock", methods }
            : { error: { code: "NOT_IMPLEMENTED", message: "mock wallet" } };
        const payload = request.method === "get_info" ? { result } : result;
        const content = await nip04.encrypt(
          sk,
          event.pubkey,
          JSON.stringify(payload),
        );
        const response = finalizeEvent(
          {
            kind: 23195,
            created_at: now(),
            tags: [
              ["p", event.pubkey],
              ["e", event.id],
            ],
            content,
          },
          sk,
        );
        ws.send(JSON.stringify(["EVENT", response]));
      } catch (e) {
        reject(e);
      }
    });

    ws.on("error", reject);
  });

  return ready;
}

export async function testAdminChannel() {
  console.log("\n--- Testing the admin channel ---");

  // Every scenario shares one private state directory so a leftover row in
  // the project's `.wrangler/state` cannot decide what a "fresh relay" lists.
  const stateDir = await mkdtemp(path.join(tmpdir(), "mekhala-admin-"));
  // The paging scenario needs a namespace larger than one page, so it gets its
  // own directory and is cleaned up with the rest.
  const pagingDir = await mkdtemp(path.join(tmpdir(), "mekhala-admin-pages-"));
  try {
  // 1. No credential configured: the channel does not exist. The relay
  // password in this scenario is deliberately valid for websocket clients.
  await withRelay(
    { port: PORT, secret: "admin-unconfigured", persistTo: stateDir },
    async () => {
    for (const suffix of ["", "/api/registrations"]) {
      const response = await fetch(adminUrl(suffix));
      check(
        response.status === 404,
        `an unconfigured admin channel must answer 404, got ${response.status} for '${suffix}'`,
      );
    }
    console.log("✅ An unconfigured admin channel answers 404.");
    },
  );

  // 2. Configured with a secret: the document is public, the data is not.
  await withRelay(
    {
      port: PORT,
      secret: "admin-auth",
      persistTo: stateDir,
      vars: { ADMIN_SECRET },
    },
    async () => {
      const page = await fetch(adminUrl(""));
      check(page.status === 200, `admin page failed: ${page.status}`);
      const pageBody = await page.text();
      check(
        pageBody.includes("Lightning Addresses"),
        "the admin page should be the console",
      );
      check(
        page.headers.get("cache-control") === "no-store",
        "the admin page must not be cached",
      );
      check(
        (page.headers.get("content-security-policy") || "").includes("nonce-"),
        "the admin page must carry a nonce CSP",
      );
      check(
        pageBody.includes('id="more"'),
        "the console should offer the next page when one exists",
      );

      const anonymous = await adminFetch("GET", "/api/registrations", {
        secret: null,
      });
      check(anonymous.status === 401, `anonymous listing: ${anonymous.status}`);
      check(
        anonymous.headers.get("cache-control") === "no-store",
        "admin API responses must not be cached",
      );

      const wrong = await adminFetch("GET", "/api/registrations", {
        secret: "not-the-secret",
      });
      check(wrong.status === 401, `wrong secret: ${wrong.status}`);

      // The relay password opens websockets; it must not open the admin API.
      const relayPassword = await adminFetch("GET", "/api/registrations", {
        secret: "admin-auth",
      });
      check(
        relayPassword.status === 401,
        `RELAY_SECRET must not be an admin credential, got ${relayPassword.status}`,
      );

      const listed = await adminFetch("GET", "/api/registrations");
      check(listed.status === 200, `authorized listing: ${listed.status}`);
      const data = await listed.json();
      check(
        Array.isArray(data.registrations) && data.registrations.length === 0,
        `a fresh relay should list nothing, got ${JSON.stringify(data)}`,
      );
      console.log("✅ Only the admin secret opens the API; RELAY_SECRET does not.");
    },
  );

    // 3. Scope check, registration, listing, deletion.
    await withRelay(
      {
        port: PORT,
        secret: "admin-register",
        persistTo: stateDir,
        vars: { ADMIN_SECRET },
      },
      async () => {
        const noPay = await mockWallet({
          relaySecret: "admin-register",
          secretByte: 7,
          methods: ["get_info", "make_invoice"],
        });
        const refused = await adminFetch("POST", "/api/registrations", {
          body: { username: "nopay", nwc_uri: connectionUri(7, "admin-register") },
        });
        check(
          refused.status === 400,
          `a wallet without pay_invoice must be refused, got ${refused.status}`,
        );
        const refusal = await refused.json();
        check(
          /pay_invoice/.test(refusal.error || ""),
          `the refusal should name the missing scope, got ${JSON.stringify(refusal)}`,
        );

        const notStored = await wellKnown("nopay");
        check(
          notStored.body.reason === "User not found",
          `a refused registration must not be stored, got ${JSON.stringify(notStored.body)}`,
        );
        console.log("✅ A wallet without pay_invoice is refused and not stored.");
        noPay.ws.close();

        const wallet = await mockWallet({
          relaySecret: "admin-register",
          secretByte: 8,
          methods: ["get_info", "make_invoice", "pay_invoice"],
        });

        const created = await adminFetch("POST", "/api/registrations", {
          body: { username: "alice_admin", nwc_uri: connectionUri(8, "admin-register") },
        });
        check(created.status === 200, `registration failed: ${created.status}`);
        const summary = await created.json();
        check(
          summary.username === "alice_admin",
          `unexpected summary: ${JSON.stringify(summary)}`,
        );
        const summaryText = JSON.stringify(summary);
        check(
          !summaryText.includes("nostr+walletconnect") &&
            !summaryText.includes(Buffer.alloc(32, 8).toString("hex")),
          "the registration summary must not echo the connection string",
        );

        const duplicate = await adminFetch("POST", "/api/registrations", {
          body: { username: "alice_admin", nwc_uri: connectionUri(9, "admin-register") },
        });
        check(
          duplicate.status === 409,
          `an existing username must not be replaced, got ${duplicate.status}`,
        );

        const listed = await adminFetch("GET", "/api/registrations");
        check(listed.status === 200, `listing failed: ${listed.status}`);
        const listing = await listed.json();
        check(
          listing.registrations.length === 1,
          `expected one registration, got ${JSON.stringify(listing)}`,
        );
        const [entry] = listing.registrations;
        check(
          entry.wallet_pubkey === walletPubkey(8),
          `the listing should name the wallet, got ${JSON.stringify(entry)}`,
        );
        check(
          (entry.methods || []).includes("pay_invoice"),
          `the listing should show the granted methods, got ${JSON.stringify(entry)}`,
        );
        check(
          !JSON.stringify(listing).includes("nostr+walletconnect"),
          "the listing must not carry connection strings",
        );
        check(
          listing.next_cursor === null,
          `a namespace that fits one page must not hand out a cursor, got ${JSON.stringify(listing)}`,
        );

        // A cursor is a page marker this relay handed out, never free-form
        // input: an oversized one is refused instead of being passed to KV.
        const oversizedCursor = await adminFetch(
          "GET",
          `/api/registrations?cursor=${"a".repeat(4096)}`,
        );
        check(
          oversizedCursor.status === 400,
          `an oversized cursor must be refused, got ${oversizedCursor.status}`,
        );

        const served = await wellKnown("alice_admin");
        check(
          served.body.tag === "payRequest",
          `a registered address should resolve, got ${JSON.stringify(served.body)}`,
        );
        console.log("✅ A registration is scope-checked, stored, and listed without secrets.");

        const removed = await adminFetch("DELETE", "/api/registrations/alice_admin");
        check(removed.status === 200, `delete failed: ${removed.status}`);
        const again = await adminFetch("DELETE", "/api/registrations/alice_admin");
        check(again.status === 404, `deleting a missing row: ${again.status}`);

        const gone = await wellKnown("alice_admin");
        check(
          gone.body.reason === "User not found",
          `a deleted address must not resolve, got ${JSON.stringify(gone.body)}`,
        );
        console.log("✅ Delete removes the address and is idempotent-safe.");
        wallet.ws.close();
      },
    );

    // 4. Bulk re-seal lets the previous key be retired.
    await withRelay(
      {
        port: PORT,
        secret: "admin-reseal",
        persistTo: stateDir,
        vars: { ADMIN_SECRET },
      },
      async () => {
        const wallet = await mockWallet({
          relaySecret: "admin-reseal",
          secretByte: 10,
          methods: ["get_info", "pay_invoice"],
        });
        const created = await adminFetch("POST", "/api/registrations", {
          body: { username: "bob_admin", nwc_uri: connectionUri(10, "admin-reseal") },
        });
        check(created.status === 200, `rotation seed failed: ${created.status}`);
        wallet.ws.close();
      },
    );

    await withRelay(
      {
        port: PORT,
        secret: "admin-reseal",
        persistTo: stateDir,
        vars: {
          ADMIN_SECRET,
          LN_ADDRESS_KEY: ROTATED_LN_ADDRESS_KEY,
          LN_ADDRESS_KEY_PREVIOUS: TEST_LN_ADDRESS_KEY,
        },
      },
      async () => {
        // Read nothing first: the point is that the endpoint alone rewrites
        // the row while the old key is still available.
        const resealed = await adminFetch("POST", "/api/reseal");
        check(resealed.status === 200, `reseal failed: ${resealed.status}`);
        const summary = await resealed.json();
        check(
          summary.resealed === 1,
          `one row should have been re-sealed, got ${JSON.stringify(summary)}`,
        );
        check(summary.failed === 0, `reseal reported failures: ${JSON.stringify(summary)}`);
        check(
          summary.next_cursor === null,
          `a namespace that fits one page must end the re-seal, got ${JSON.stringify(summary)}`,
        );
      },
    );

    // Only the new key remains, so a row that was not rewritten cannot open.
    await withRelay(
      {
        port: PORT,
        secret: "admin-reseal",
        persistTo: stateDir,
        vars: { ADMIN_SECRET, LN_ADDRESS_KEY: ROTATED_LN_ADDRESS_KEY },
      },
      async () => {
        const served = await wellKnown("bob_admin");
        check(
          served.body.tag === "payRequest",
          `a re-sealed address must survive dropping the old key, got ${JSON.stringify(served.body)}`,
        );
      },
    );
    console.log("✅ Bulk re-seal moves rows to the current key.");

    // 5. Paging: rows past the first page stay reachable. One page is 100
    // keys, so 101 registrations put the last row on a second page — the row
    // a fixed row cap used to make unreachable.
    const pageUsers = 101;
    const pagingVars = { ADMIN_SECRET, LN_ADDRESS_KEY: TEST_LN_ADDRESS_KEY };
    await withRelay(
      { port: PORT, secret: "admin-pages", persistTo: pagingDir, vars: pagingVars },
      async () => {
        const wallet = await mockWallet({
          relaySecret: "admin-pages",
          secretByte: 11,
          methods: ["get_info", "pay_invoice"],
        });
        for (let index = 0; index < pageUsers; index += 1) {
          const username = `page_${String(index).padStart(3, "0")}`;
          const created = await adminFetch("POST", "/api/registrations", {
            body: { username, nwc_uri: connectionUri(11, "admin-pages") },
          });
          check(created.status === 200, `seeding ${username} failed: ${created.status}`);
        }
        wallet.ws.close();

        const first = await (await adminFetch("GET", "/api/registrations")).json();
        check(
          first.registrations.length === 100,
          `the first page should hold 100 rows, got ${first.registrations.length}`,
        );
        check(
          typeof first.next_cursor === "string",
          `a namespace larger than one page must hand out a cursor, got ${JSON.stringify(first.next_cursor)}`,
        );

        const second = await (
          await adminFetch(
            "GET",
            `/api/registrations?cursor=${encodeURIComponent(first.next_cursor)}`,
          )
        ).json();
        check(
          second.registrations.length === 1,
          `the second page should hold the last row, got ${second.registrations.length}`,
        );
        check(
          second.next_cursor === null,
          `the last page must end the listing, got ${JSON.stringify(second.next_cursor)}`,
        );

        // Re-sealing obeys the same page contract: one call visits one page and
        // says where to continue.
        const resealPage = await (await adminFetch("POST", "/api/reseal")).json();
        check(
          resealPage.scanned === 100 && typeof resealPage.next_cursor === "string",
          `one re-seal call should visit one page, got ${JSON.stringify(resealPage)}`,
        );
        console.log("✅ Paging reaches rows past the first page.");
      },
    );

    // 6. A rotation driven through the cursor re-seals every page, including
    // the row only the second page knows about.
    await withRelay(
      {
        port: PORT,
        secret: "admin-pages",
        persistTo: pagingDir,
        vars: {
          ADMIN_SECRET,
          LN_ADDRESS_KEY: ROTATED_LN_ADDRESS_KEY,
          LN_ADDRESS_KEY_PREVIOUS: TEST_LN_ADDRESS_KEY,
        },
      },
      async () => {
        const totals = { scanned: 0, resealed: 0, failed: 0 };
        let cursor = null;
        do {
          const suffix = "/api/reseal" +
            (cursor ? `?cursor=${encodeURIComponent(cursor)}` : "");
          const response = await adminFetch("POST", suffix);
          check(response.status === 200, `re-seal page failed: ${response.status}`);
          const summary = await response.json();
          totals.scanned += summary.scanned;
          totals.resealed += summary.resealed;
          totals.failed += summary.failed;
          cursor = summary.next_cursor;
        } while (cursor);

        check(
          totals.scanned === pageUsers && totals.resealed === pageUsers,
          `every row should have been re-sealed, got ${JSON.stringify(totals)}`,
        );
        check(totals.failed === 0, `re-seal reported failures: ${JSON.stringify(totals)}`);
      },
    );

    await withRelay(
      {
        port: PORT,
        secret: "admin-pages",
        persistTo: pagingDir,
        vars: { ADMIN_SECRET, LN_ADDRESS_KEY: ROTATED_LN_ADDRESS_KEY },
      },
      async () => {
        // page_100 is the second page's only row; the first page's row proves
        // the walk did not stop after one page either.
        for (const username of ["page_000", "page_100"]) {
          const served = await wellKnown(username);
          check(
            served.body.tag === "payRequest",
            `a row must survive the rotation, ${username} gave ${JSON.stringify(served.body)}`,
          );
        }
      },
    );
    console.log("✅ A cursor-driven rotation re-seals every page.");
  } finally {
    await rm(stateDir, { recursive: true, force: true });
    await rm(pagingDir, { recursive: true, force: true });
  }
}
