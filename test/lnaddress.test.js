import { WebSocket } from "ws";
import { RELAY_URL, HTTP_URL } from "./env.js";
import { nip04 } from "nostr-tools";
import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";

export async function testLnAddressFlow() {
  console.log("\n--- Testing LN Address Flow (LUD-06 / LUD-16) ---");

  const username = "testuser_relay";
  const walletSk = new Uint8Array(32).fill(1);
  const walletPk = getPublicKey(walletSk);
  const nwcSecret = "0303030303030303030303030303030303030303030303030303030303030303";

  const nwcUri = `nostr+walletconnect://${walletPk}?relay=${encodeURIComponent(RELAY_URL)}&secret=${nwcSecret}`;

  console.log(`Using pre-configured KV: ${username} -> ${nwcUri}`);

  const wellKnownUrl = `${HTTP_URL.replace(/\/$/, "")}/.well-known/lnurlp/${username}`;
  const response = await fetch(wellKnownUrl);

  if (response.status !== 200) {
    throw new Error(`LNURLp well-known failed: ${response.status}`);
  }

  const data = await response.json();
  console.log("✅ LNURLp well-known passed:", data.callback);

  const walletWs = new WebSocket(RELAY_URL);
  const invoice = "lnbc1test_relay_invoice...";

  const walletReady = new Promise((resolve, reject) => {
    walletWs.on("open", () => {
      const infoEvent = finalizeEvent({
        kind: 13194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [],
        content: "make_invoice"
      }, walletSk);
      walletWs.send(JSON.stringify(["EVENT", infoEvent]));
      walletWs.send(JSON.stringify(["REQ", "wallet-ln-sub", { kinds: [23194], "#p": [walletPk] }]));
    });
    walletWs.on("message", async (msgData) => {
      const msg = JSON.parse(msgData.toString());
      if (msg[0] === "EOSE") resolve();
      if (msg[0] === "EVENT" && msg[1] === "wallet-ln-sub") {
        const event = msg[2];
        console.log("Mock Wallet received NWC Request from relay.");

        const decrypted = await nip04.decrypt(walletSk, event.pubkey, event.content);
        const req = JSON.parse(decrypted);

        if (req.method === "make_invoice") {
          const resp = JSON.stringify({ result: { invoice } });
          const encryptedResp = await nip04.encrypt(walletSk, event.pubkey, resp);

          const resEvent = finalizeEvent({
            kind: 23195,
            created_at: Math.floor(Date.now() / 1000),
            tags: [["p", event.pubkey], ["e", event.id]],
            content: encryptedResp
          }, walletSk);
          walletWs.send(JSON.stringify(["EVENT", resEvent]));
        }
      }
    });
    setTimeout(() => reject(new Error("Mock Wallet timeout")), 5000);
  });

  await walletReady;

  console.log("Calling callback URL...");
  const callbackUrl = `${data.callback}?amount=21000`;
  const callbackResp = await fetch(callbackUrl);

  if (callbackResp.status !== 200) {
    const err = await callbackResp.text();
    throw new Error(`LN Address callback failed: ${callbackResp.status} - ${err}`);
  }

  const callbackData = await callbackResp.json();
  if (callbackData.pr === invoice) {
    console.log("✅ LN Address flow (Well-known -> Callback -> NWC) passed!");
  } else {
    console.error("Callback Response:", callbackData);
    throw new Error(`Invoice mismatch: expected ${invoice}, got ${callbackData.pr}`);
  }

  walletWs.close();
}

export async function testLnAddressOffline() {
  console.log("\n--- Testing LN Address Offline Error Handling ---");
  const username = "offline_user";
  const walletSk = new Uint8Array(32).fill(2);
  const walletPk = getPublicKey(walletSk);

  const wellKnownUrl = `${HTTP_URL.replace(/\/$/, "")}/.well-known/lnurlp/${username}`;
  const response = await fetch(wellKnownUrl);
  const data = await response.json();

  console.log("Calling callback URL for offline wallet...");
  const callbackUrl = `${data.callback}?amount=21000`;
  const callbackResp = await fetch(callbackUrl);

  if (callbackResp.status === 200) {
    const errData = await callbackResp.json();
    if (errData.status === "ERROR" && errData.reason && errData.reason.includes("Wallet not connected")) {
      console.log("✅ LN Address offline error handled correctly.");
      return;
    }
    throw new Error(`Expected 'Wallet not connected' error, but got: 200 - ${JSON.stringify(errData)}`);
  }

  throw new Error(`Expected 200 error, but got: ${callbackResp.status}`);
}

export async function testLnAddressErrors() {
  console.log("\n--- Testing LN Address Error Handling ---");

  const unknownUser = "nonexistent_user_" + Date.now();
  const wellKnownUrl = `${HTTP_URL.replace(/\/$/, "")}/.well-known/lnurlp/${unknownUser}`;
  const response = await fetch(wellKnownUrl);

  if (response.status !== 200) {
    throw new Error(`Expected 200 for unknown user well-known, got ${response.status}`);
  }
  const data = await response.json();
  if (data.status !== "ERROR" || !data.reason || !data.reason.includes("not found")) {
    throw new Error(`Expected ERROR with 'not found', got: ${JSON.stringify(data)}`);
  }
  console.log("✅ Unknown user well-known returns ERROR.");

  const callbackUrl = `${HTTP_URL.replace(/\/$/, "")}/lnaddress/testuser_relay/callback`;

  const respNoAmount = await fetch(callbackUrl);
  const dataNoAmount = await respNoAmount.json();
  if (dataNoAmount.status !== "ERROR" || !dataNoAmount.reason || !dataNoAmount.reason.includes("Missing amount")) {
    throw new Error(`Expected ERROR 'Missing amount', got: ${JSON.stringify(dataNoAmount)}`);
  }
  console.log("✅ Callback without amount returns 'Missing amount' ERROR.");

  const respNonNumeric = await fetch(`${callbackUrl}?amount=abc`);
  const dataNonNumeric = await respNonNumeric.json();
  if (dataNonNumeric.status !== "ERROR" || !dataNonNumeric.reason || !dataNonNumeric.reason.includes("Missing amount")) {
    throw new Error(`Expected ERROR 'Missing amount' for non-numeric, got: ${JSON.stringify(dataNonNumeric)}`);
  }
  console.log("✅ Callback with non-numeric amount returns 'Missing amount' ERROR.");

  const respNegative = await fetch(`${callbackUrl}?amount=-100`);
  const dataNegative = await respNegative.json();
  if (dataNegative.status !== "ERROR" || !dataNegative.reason || !dataNegative.reason.includes("Missing amount")) {
    throw new Error(`Expected ERROR 'Missing amount' for negative, got: ${JSON.stringify(dataNegative)}`);
  }
  console.log("✅ Callback with negative amount returns 'Missing amount' ERROR.");
}
