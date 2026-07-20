import { WebSocket } from "ws";
import * as nostr from "nostr-tools";
import { nip04, nip44 } from "nostr-tools";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

const args = process.argv.slice(2);
let baseURL = args[0] || "localhost:8787";
const relaySecret = args[1] !== undefined ? args[1] : "";

baseURL = baseURL
  .replace(/^https?:\/\//, "")
  .replace(/^wss?:\/\//, "")
  .replace(/\/$/, "");

export const isLocal = baseURL.includes("localhost") || baseURL.includes("127.0.0.1");
const wsProtocol = isLocal ? "ws://" : "wss://";
const httpProtocol = isLocal ? "http://" : "https://";

export const RELAY_URL = `${wsProtocol}${baseURL}/${relaySecret}`;
export const HTTP_URL = `${httpProtocol}${baseURL}/${relaySecret}`;

console.log(`Testing against:`);
console.log(`  WebSocket: ${RELAY_URL}`);
console.log(`  HTTP:      ${HTTP_URL}\n`);

export { baseURL, httpProtocol, relaySecret };

export async function setupTempKV(username, nwcUri) {
  const { execSync } = await import("child_process");
  const configFile = process.env.WRANGLER_CONFIG || "wrangler.toml";
  const configArg = configFile.startsWith('/') ? configFile : `../${configFile}`;
  execSync(`npx wrangler kv key put --binding MEKHALA_NWC_KV --local --config ${configArg} "${username}" "${nwcUri}"`);
}

export function hex(bytes) {
  return Buffer.from(bytes).toString("hex");
}
