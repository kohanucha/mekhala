// Storage layout for Lightning Address registrations.
//
// A stored NWC URI is a bearer credential, so records written from now on are
// sealed under a key derived from `LN_ADDRESS_KEY`. These scenarios cover what
// the Rust unit tests cannot: the KV layout itself, the one-time move of
// plaintext rows left by earlier versions, key rotation, and the refusal to
// serve anything when no usable key is configured.
//
// The whole file runs against one private `--persist-to` directory, stopping
// and restarting the relay between scenarios. The KV rows are inspected with
// the wrangler CLI only while no relay is running, so the two processes never
// contend for the same state.

import { execFileSync } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { getPublicKey } from "nostr-tools/pure";
import { withRelay, TEST_LN_ADDRESS_KEY } from "./relay-harness.js";

const projectRoot = new URL("..", import.meta.url).pathname;
const configPath = path.join(projectRoot, "wrangler.toml");
const testDir = path.join(projectRoot, "test");

/** A second valid key, used as the new key in the rotation scenario. */
const ROTATED_LN_ADDRESS_KEY = "c3".repeat(32);

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

/** Runs one `wrangler kv` command against the private state directory. */
function kv(persistTo, args) {
  return execFileSync(
    "npx",
    [
      "wrangler",
      "kv",
      ...args,
      "--binding",
      "MEKHALA_NWC_KV",
      "--local",
      "--persist-to",
      persistTo,
      "--config",
      configPath,
    ],
    { cwd: testDir, encoding: "utf8" },
  );
}

function seedPlaintext(persistTo, username, nwcUri) {
  kv(persistTo, ["key", "put", username, nwcUri]);
}

function listKeys(persistTo) {
  return JSON.parse(kv(persistTo, ["key", "list"])).map((entry) => entry.name);
}

function getValue(persistTo, key) {
  return kv(persistTo, ["key", "get", key]).trim();
}

function nwcUri(secretByte, port, secret) {
  const walletPk = getPublicKey(new Uint8Array(32).fill(secretByte));
  const nwcSecret = Buffer.alloc(32, secretByte).toString("hex");
  const relay = encodeURIComponent(`ws://127.0.0.1:${port}/${secret}`);
  return `nostr+walletconnect://${walletPk}?relay=${relay}&secret=${nwcSecret}`;
}

async function wellKnown(port, username) {
  const resp = await fetch(`http://127.0.0.1:${port}/.well-known/lnurlp/${username}`);
  return { status: resp.status, body: await resp.json() };
}

export async function testLnAddressStorage() {
  console.log("\n--- Testing LN address record storage ---");

  await withStateDir("lnaddr-store", async (persistTo) => {
    const secret = "storage-test-secret";
    const migratedUser = "migrated_user";
    const legacyUser = "legacy_user";

    seedPlaintext(persistTo, migratedUser, nwcUri(11, 8797, secret));
    seedPlaintext(persistTo, legacyUser, nwcUri(12, 8797, secret));

    // 1. A plaintext row from an earlier version is served and re-sealed.
    await withRelay({ port: 8797, secret, persistTo }, async () => {
      const resp = await wellKnown(8797, migratedUser);
      check(
        resp.body.tag === "payRequest",
        `a plaintext row must still be served, got ${JSON.stringify(resp.body)}`,
      );
    });

    let keys = listKeys(persistTo);
    check(
      keys.includes(`lnaddr:${migratedUser}`),
      "the record should have been written under lnaddr:<username>",
    );
    check(
      !keys.includes(migratedUser),
      "the plaintext row should have been removed after migration",
    );

    const sealed = getValue(persistTo, `lnaddr:${migratedUser}`);
    check(sealed.startsWith("v1:"), `the stored record must be sealed, got '${sealed.slice(0, 4)}...'`);
    check(
      !sealed.includes("nostr+walletconnect"),
      "the stored record must not contain the connection URI",
    );
    check(
      !sealed.includes(migratedUser),
      "the stored record must not contain the username in clear text",
    );
    console.log("✅ A plaintext row is re-sealed on first read and removed.");

    // 2. A record sealed under the old key keeps working during rotation.
    await withRelay(
      {
        port: 8799,
        secret,
        persistTo,
        vars: {
          LN_ADDRESS_KEY: ROTATED_LN_ADDRESS_KEY,
          LN_ADDRESS_KEY_PREVIOUS: TEST_LN_ADDRESS_KEY,
        },
      },
      async () => {
        const resp = await wellKnown(8799, migratedUser);
        check(
          resp.body.tag === "payRequest",
          `the previous key must keep old records readable, got ${JSON.stringify(resp.body)}`,
        );
      },
    );
    console.log("✅ A record written under the previous key still opens.");

    // 3. With no usable key, neither layout is served, and nothing is deleted.
    await withRelay(
      { port: 8798, secret, persistTo, vars: { LN_ADDRESS_KEY: "not-a-key" } },
      async ({ log }) => {
        for (const username of [migratedUser, legacyUser]) {
          const resp = await wellKnown(8798, username);
          check(
            resp.body.status === "ERROR" && resp.body.reason === "User not found",
            `${username} must not be served without a usable key, got ${JSON.stringify(resp.body)}`,
          );
        }
        check(
          log().includes("LN_ADDRESS_KEY"),
          "a malformed key should be reported in the relay log",
        );
      },
    );

    keys = listKeys(persistTo);
    check(
      keys.includes(`lnaddr:${migratedUser}`),
      "a relay without a usable key must not delete sealed records",
    );
    check(
      keys.includes(legacyUser),
      "a relay without a usable key must not drop the plaintext row",
    );
    console.log("✅ Without a usable key, sealed and plaintext records are both refused.");
  });
}
