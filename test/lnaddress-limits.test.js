// LN address callback policy: the advertised sendable range and the
// per-source callback rate limit.
//
// The Rust unit tests cover the policy and window-logic in isolation. These two
// scenarios exercise what they cannot: that the range the relay advertises on
// `.well-known` is the range the callback enforces, and that the rate limit is
// keyed on the address the edge reports (the Worker stamps it on the callback
// before forwarding to the Durable Object).
//
// Each scenario runs its own relay with a private `--persist-to` directory and
// seeds the KV binding there, so the main suite's `.wrangler/state` and other
// tests cannot decide which usernames exist.

import { execFileSync } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { getPublicKey } from "nostr-tools/pure";
import { withRelay } from "./relay-harness.js";

const projectRoot = new URL("..", import.meta.url).pathname;
const configPath = path.join(projectRoot, "wrangler.toml");

function check(condition, message) {
  if (!condition) throw new Error(message);
}

async function withStateDir(name, fn) {
  const dir = await mkdtemp(path.join(tmpdir(), `mekhala-${name}-`));
  try {
    return await fn(dir);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

/** Writes a username -> NWC URI entry into the private KV state directory. */
function seedUser(persistTo, port, secret, username, secretByte) {
  const walletPk = getPublicKey(new Uint8Array(32).fill(secretByte));
  const nwcSecret = Buffer.alloc(32, secretByte).toString("hex");
  const relay = encodeURIComponent(`ws://127.0.0.1:${port}/${secret}`);
  const nwcUri = `nostr+walletconnect://${walletPk}?relay=${relay}&secret=${nwcSecret}`;

  execFileSync(
    "npx",
    [
      "wrangler",
      "kv",
      "key",
      "put",
      "--binding",
      "MEKHALA_NWC_KV",
      "--local",
      "--persist-to",
      persistTo,
      "--config",
      configPath,
      username,
      nwcUri,
    ],
    { cwd: path.join(projectRoot, "test"), stdio: "inherit" },
  );
}

function callbackURL(port, username) {
  return `http://127.0.0.1:${port}/lnaddress/${username}/callback`;
}

async function requestCallback(port, username, amount, headers = {}) {
  const resp = await fetch(`${callbackURL(port, username)}?amount=${amount}`, {
    headers,
  });
  return { status: resp.status, body: await resp.json() };
}

export async function testLnAddressAmountBounds() {
  console.log("\n--- Testing the LN address sendable range ---");

  await withStateDir("lnaddr-bounds", async (persistTo) => {
    const port = 8795;
    const secret = "bounds-test-secret";
    const username = "bounds_user";
    // The rate limit is exercised by its own scenario; keep it off here so a
    // shared source cannot decide these results.
    const vars = {
      MIN_AMOUNT_MSAT: 5000,
      MAX_AMOUNT_MSAT: 50000,
      CALLBACK_RATE_LIMIT_PER_MINUTE: 0,
    };

    seedUser(persistTo, port, secret, username, 7);

    await withRelay({ port, vars, secret, persistTo }, async () => {
      const wellKnown = await fetch(
        `http://127.0.0.1:${port}/.well-known/lnurlp/${username}`,
      );
      check(wellKnown.status === 200, `well-known failed: ${wellKnown.status}`);
      const info = await wellKnown.json();
      check(
        info.minSendable === 5000,
        `well-known must advertise minSendable=5000, got ${info.minSendable}`,
      );
      check(
        info.maxSendable === 50000,
        `well-known must advertise maxSendable=50000, got ${info.maxSendable}`,
      );
      console.log("✅ Well-known advertises the configured range.");

      const below = await requestCallback(port, username, 4999);
      check(below.status === 200, `below-min callback must be 200, got ${below.status}`);
      check(
        below.body.status === "ERROR" &&
          below.body.reason === "Amount below minimum (5000 msat)",
        `expected the minimum refusal, got ${JSON.stringify(below.body)}`,
      );
      console.log("✅ Just below the minimum is refused with the advertised bound.");

      const above = await requestCallback(port, username, 50001);
      check(above.status === 200, `above-max callback must be 200, got ${above.status}`);
      check(
        above.body.status === "ERROR" &&
          above.body.reason === "Amount above maximum (50000 msat)",
        `expected the maximum refusal, got ${JSON.stringify(above.body)}`,
      );
      console.log("✅ Just above the maximum is refused with the advertised bound.");

      // Both ends of the advertised range are accepted, which is what LUD-06
      // promises. No wallet is connected here, so the in-range check is proven
      // by reaching the wallet gate rather than by minting an invoice.
      for (const amount of [5000, 50000]) {
        const inRange = await requestCallback(port, username, amount);
        check(
          inRange.body.status === "ERROR" &&
            inRange.body.reason === "Wallet not connected",
          `amount ${amount} must pass the range check and reach the wallet, got ${JSON.stringify(inRange.body)}`,
        );
      }
      console.log("✅ Both endpoints of the range reach the wallet.");
    });
  });
}

export async function testLnAddressCallbackRateLimit() {
  console.log("\n--- Testing the LN address callback rate limit ---");

  await withStateDir("lnaddr-rate", async (persistTo) => {
    const port = 8796;
    const secret = "rate-test-secret";
    const username = "rate_user";
    const vars = { CALLBACK_RATE_LIMIT_PER_MINUTE: 2 };

    seedUser(persistTo, port, secret, username, 8);

    await withRelay({ port, vars, secret, persistTo }, async () => {
      const source = { "CF-Connecting-IP": "203.0.113.7" };

      // Admitted callbacks proceed to the wallet, which is offline, so a
      // "Wallet not connected" answer is the observable proof of admission.
      for (let i = 1; i <= 2; i++) {
        const resp = await requestCallback(port, username, 21000, source);
        check(
          resp.body.status === "ERROR" && resp.body.reason === "Wallet not connected",
          `callback ${i} should be admitted, got ${JSON.stringify(resp.body)}`,
        );
      }
      console.log("✅ Two callbacks within the limit are admitted.");

      const limited = await requestCallback(port, username, 21000, source);
      check(
        limited.status === 200,
        `a limited callback must still be 200, got ${limited.status}`,
      );
      check(
        limited.body.status === "ERROR" &&
          /^Too many requests, retry in \d+s$/.test(limited.body.reason ?? ""),
        `expected the rate-limit refusal, got ${JSON.stringify(limited.body)}`,
      );
      console.log(`✅ The third callback is refused: "${limited.body.reason}".`);

      const other = await requestCallback(port, username, 21000, {
        "CF-Connecting-IP": "203.0.113.8",
      });
      check(
        other.body.status === "ERROR" && other.body.reason === "Wallet not connected",
        `a different source must not share the counter, got ${JSON.stringify(other.body)}`,
      );
      console.log("✅ A different source has its own counter.");

      // Without an address tag the Worker has nothing to key a counter on, so
      // the callback is admitted (the same rule per-address connection limits
      // follow).
      const untagged = await requestCallback(port, username, 21000);
      check(
        untagged.body.status === "ERROR" && untagged.body.reason === "Wallet not connected",
        `a callback with no address tag must be admitted, got ${JSON.stringify(untagged.body)}`,
      );
      console.log("✅ A callback with no address tag is admitted.");
    });
  });
}
