import { WebSocket } from "ws";
import { RELAY_URL } from "./env.js";
import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";

export async function testNip09Deletion() {
  console.log("\n--- Testing NIP-09 Deletion (Kind 5) ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);

  const walletWs = new WebSocket(RELAY_URL);
  let infoEvent;
  await new Promise((resolve, reject) => {
    walletWs.on("open", () => {
      infoEvent = finalizeEvent(
        {
          kind: 13194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [],
          content: "nip09_deletion_test",
        },
        walletSk,
      );
      walletWs.send(JSON.stringify(["EVENT", infoEvent]));
    });

    walletWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === true) {
        console.log("✅ Wallet received OK for Info Event (13194).");
        resolve();
      }
    });

    walletWs.on("error", reject);
    setTimeout(() => reject(new Error("Wallet publish timeout waiting for OK")), 5000);
  });

  await new Promise((resolve, reject) => {
    const deletionEvent = finalizeEvent(
      {
        kind: 5,
        created_at: Math.floor(Date.now() / 1000),
        tags: [["e", infoEvent.id]],
        content: "wallet deleted",
      },
      walletSk,
    );
    walletWs.send(JSON.stringify(["EVENT", deletionEvent]));

    walletWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === true && msg[1] === deletionEvent.id) {
        console.log("✅ Wallet received OK for Deletion Event (kind 5).");
        resolve();
      }
    });

    setTimeout(() => reject(new Error("Deletion event publish timeout waiting for OK")), 5000);
  });

  const appWs = new WebSocket(RELAY_URL);
  let infoReceived = false;
  await new Promise((resolve, reject) => {
    appWs.on("open", () => {
      appWs.send(
        JSON.stringify([
          "REQ",
          "nip09-sub",
          { kinds: [13194], authors: [walletPk] },
        ]),
      );
    });

    appWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EVENT" && msg[1] === "nip09-sub") {
        if (msg[2].kind === 13194 && msg[2].content === "nip09_deletion_test") {
          infoReceived = true;
        }
      }
      if (msg[0] === "EOSE" && msg[1] === "nip09-sub") {
        setTimeout(() => {
          if (!infoReceived) {
            console.log("✅ App received EOSE with no info event after NIP-09 deletion.");
            appWs.close();
            resolve();
          } else {
            reject(new Error("Info event still returned after NIP-09 deletion"));
          }
        }, 500);
      }
    });

    appWs.on("error", reject);
    setTimeout(() => reject(new Error("App NIP-09 test timeout")), 5000);
  });

  walletWs.close();
}

export async function testNip09KTagDeletion() {
  console.log("\n--- Testing NIP-09 Deletion (Kind 5) with k-tag ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const walletWs = new WebSocket(RELAY_URL);

  await new Promise((resolve, reject) => {
    walletWs.on("open", () => {
      const infoEvent = finalizeEvent({
        kind: 13194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [],
        content: "ktag_deletion_test",
      }, walletSk);
      walletWs.send(JSON.stringify(["EVENT", infoEvent]));
    });
    walletWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === true) resolve();
    });
    walletWs.on("error", reject);
    setTimeout(() => reject(new Error("Wallet publish timeout")), 5000);
  });

  await new Promise((resolve, reject) => {
    const deletionEvent = finalizeEvent({
      kind: 5,
      created_at: Math.floor(Date.now() / 1000),
      tags: [["k", "13194"]],
      content: "",
    }, walletSk);
    walletWs.send(JSON.stringify(["EVENT", deletionEvent]));
    walletWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === true && msg[1] === deletionEvent.id) resolve();
    });
    setTimeout(() => reject(new Error("Deletion timeout")), 5000);
  });

  const appWs = new WebSocket(RELAY_URL);
  let infoReceived = false;
  await new Promise((resolve, reject) => {
    appWs.on("open", () => {
      appWs.send(JSON.stringify(["REQ", "ktag-sub", { kinds: [13194], authors: [walletPk] }]));
    });
    appWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EVENT" && msg[1] === "ktag-sub") infoReceived = true;
      if (msg[0] === "EOSE" && msg[1] === "ktag-sub") {
        setTimeout(() => {
          if (!infoReceived) {
            console.log("✅ k-tag deletion verified.");
            appWs.close();
            resolve();
          } else {
            reject(new Error("Info event still present after k-tag deletion"));
          }
        }, 500);
      }
    });
    appWs.on("error", reject);
    setTimeout(() => reject(new Error("k-tag check timeout")), 5000);
  });

  walletWs.close();
}

export async function testNip09BlanketDeletion() {
  console.log("\n--- Testing NIP-09 Blanket Deletion (Kind 5, no tags) ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const walletWs = new WebSocket(RELAY_URL);

  await new Promise((resolve, reject) => {
    walletWs.on("open", () => {
      const infoEvent = finalizeEvent({
        kind: 13194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [],
        content: "blanket_deletion_test",
      }, walletSk);
      walletWs.send(JSON.stringify(["EVENT", infoEvent]));
    });
    walletWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === true) resolve();
    });
    walletWs.on("error", reject);
    setTimeout(() => reject(new Error("Wallet publish timeout")), 5000);
  });

  await new Promise((resolve, reject) => {
    const deletionEvent = finalizeEvent({
      kind: 5,
      created_at: Math.floor(Date.now() / 1000),
      tags: [],
      content: "",
    }, walletSk);
    walletWs.send(JSON.stringify(["EVENT", deletionEvent]));
    walletWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === true && msg[1] === deletionEvent.id) resolve();
    });
    setTimeout(() => reject(new Error("Deletion timeout")), 5000);
  });

  const appWs = new WebSocket(RELAY_URL);
  let infoReceived = false;
  await new Promise((resolve, reject) => {
    appWs.on("open", () => {
      appWs.send(JSON.stringify(["REQ", "blanket-sub", { kinds: [13194], authors: [walletPk] }]));
    });
    appWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EVENT" && msg[1] === "blanket-sub") infoReceived = true;
      if (msg[0] === "EOSE" && msg[1] === "blanket-sub") {
        setTimeout(() => {
          if (!infoReceived) {
            console.log("✅ Blanket deletion verified.");
            appWs.close();
            resolve();
          } else {
            reject(new Error("Info event still present after blanket deletion"));
          }
        }, 500);
      }
    });
    appWs.on("error", reject);
    setTimeout(() => reject(new Error("Blanket check timeout")), 5000);
  });

  walletWs.close();
}

export async function testNip09UnauthorizedDeletion() {
  console.log("\n--- Testing NIP-09 Unauthorized Deletion (wrong pubkey) ---");

  const walletSk = generateSecretKey();
  const walletPk = getPublicKey(walletSk);
  const attackerSk = generateSecretKey();
  const walletWs = new WebSocket(RELAY_URL);
  let infoEventId;

  await new Promise((resolve, reject) => {
    walletWs.on("open", () => {
      const infoEvent = finalizeEvent({
        kind: 13194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [],
        content: "unauthorized_delete_test",
      }, walletSk);
      infoEventId = infoEvent.id;
      walletWs.send(JSON.stringify(["EVENT", infoEvent]));
    });
    walletWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === true) resolve();
    });
    walletWs.on("error", reject);
    setTimeout(() => reject(new Error("Wallet publish timeout")), 5000);
  });

  const deletionEvent = finalizeEvent({
    kind: 5,
    created_at: Math.floor(Date.now() / 1000),
    tags: [["e", infoEventId]],
    content: "malicious deletion",
  }, attackerSk);
  walletWs.send(JSON.stringify(["EVENT", deletionEvent]));

  await new Promise((resolve, reject) => {
    walletWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[1] === deletionEvent.id) {
        resolve();
      }
    });
    setTimeout(() => reject(new Error("Attacker deletion timeout")), 5000);
  });

  const appWs = new WebSocket(RELAY_URL);
  let infoReceived = false;
  await new Promise((resolve, reject) => {
    appWs.on("open", () => {
      appWs.send(JSON.stringify(["REQ", "unauth-sub", { kinds: [13194], authors: [walletPk] }]));
    });
    appWs.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EVENT" && msg[1] === "unauth-sub" && msg[2].content === "unauthorized_delete_test") {
        infoReceived = true;
      }
      if (msg[0] === "EOSE" && msg[1] === "unauth-sub") {
        setTimeout(() => {
          if (infoReceived) {
            console.log("✅ Unauthorized deletion correctly rejected.");
            appWs.close();
            resolve();
          } else {
            reject(new Error("Info event was deleted despite unauthorized attempt"));
          }
        }, 500);
      }
    });
    appWs.on("error", reject);
    setTimeout(() => reject(new Error("Unauthorized check timeout")), 5000);
  });

  walletWs.close();
}
