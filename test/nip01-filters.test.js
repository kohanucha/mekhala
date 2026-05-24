import { WebSocket } from "ws";
import { RELAY_URL } from "./env.js";
import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";

export async function testNip01EdgeCases() {
  console.log("\n--- Testing NIP-01 Edge Cases (Adapted for Strict NWC) ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  return new Promise((resolve, reject) => {
    ws.on("open", () => {
      console.log("Step 1: Testing ID mismatch...");
      const event = finalizeEvent(
        {
          kind: 23194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", pk]],
          content: "test",
        },
        sk,
      );
      event.id = "0".repeat(64);
      ws.send(JSON.stringify(["EVENT", event]));

      console.log("Step 2: Testing malformed JSON...");
      ws.send("not a json array");

      console.log("Step 3: Testing message size limit...");
      const largeContent = "a".repeat(140000);
      ws.send(JSON.stringify(["EVENT", { content: largeContent }]));

      console.log("Step 4: Testing multiple filters in REQ...");
      ws.send(
        JSON.stringify([
          "REQ",
          "multi-sub",
          { kinds: [23194], authors: [pk] },
          { kinds: [13194], "#p": [pk] },
        ]),
      );
    });

    let idMismatchRejected = false;
    let malformedJsonRejected = false;
    let largeMessageNotice = false;
    let multiSubEose = false;
    let closeTestPassed = false;

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());

      if (msg[0] === "OK" && msg[2] === false && msg[3]?.includes("id")) {
        console.log("✅ ID mismatch rejected.");
        idMismatchRejected = true;
      }

      if (msg[0] === "NOTICE" && msg[1]?.includes("parse failed")) {
        console.log("✅ Malformed JSON rejected.");
        malformedJsonRejected = true;
      }

      if (msg[0] === "NOTICE" && msg[1]?.includes("too large")) {
        console.log("✅ Large message notice received.");
        largeMessageNotice = true;
      }

      if (msg[0] === "EOSE" && msg[1] === "multi-sub") {
        console.log("✅ Multiple filters REQ accepted.");
        multiSubEose = true;

        console.log("Step 5: Testing CLOSE functionality...");
        ws.send(JSON.stringify(["CLOSE", "multi-sub"]));

        const matchEvent = finalizeEvent(
          {
            kind: 23194,
            created_at: Math.floor(Date.now() / 1000),
            tags: [["p", pk]],
            content: "after close",
          },
          sk,
        );
        ws.send(JSON.stringify(["EVENT", matchEvent]));

        setTimeout(() => {
          closeTestPassed = true;
          checkDone();
        }, 1000);
      }

      if (msg[0] === "EVENT" && msg[1] === "multi-sub") {
        reject(new Error("Received event for closed subscription!"));
      }

      checkDone();
    });

    const checkDone = () => {
      if (
        idMismatchRejected &&
        malformedJsonRejected &&
        largeMessageNotice &&
        multiSubEose &&
        closeTestPassed
      ) {
        ws.close();
        resolve();
      }
    };

    setTimeout(() => {
      reject(
        new Error(
          `NIP-01 Edge Cases Timeout. ID: ${idMismatchRejected}, JSON: ${malformedJsonRejected}, Large: ${largeMessageNotice}, Multi: ${multiSubEose}, Close: ${closeTestPassed}`,
        ),
      );
    }, 10000);
  });
}

export async function testFilterMatching() {
  console.log("\n--- Testing Filter Matching (#e tags, since, until) ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  return new Promise((resolve, reject) => {
    let eTagReceived = false;
    let sinceReceived = false;
    let untilReceived = false;

    ws.on("open", () => {
      const parentEvent = finalizeEvent({
        kind: 23194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [["p", pk], ["e", "test-event-id-123"]],
        content: "event with e-tag"
      }, sk);

      ws.send(JSON.stringify(["REQ", "sub-e", { kinds: [23194], "#e": ["test-event-id-123"], "#p": [pk] }]));
    });

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());

      if (msg[0] === "EOSE" && msg[1] === "sub-e") {
        const eventWithETag = finalizeEvent({
          kind: 23194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", pk], ["e", "test-event-id-123"]],
          content: "matches e-tag"
        }, sk);
        ws.send(JSON.stringify(["EVENT", eventWithETag]));
      }

      if (msg[0] === "EVENT" && msg[1] === "sub-e") {
        if (msg[2].tags.some(t => t[0] === "e" && t[1] === "test-event-id-123")) {
          eTagReceived = true;
          console.log("✅ Filter matching on #e tag works.");
          ws.send(JSON.stringify(["CLOSE", "sub-e"]));

          const now = Math.floor(Date.now() / 1000);
          ws.send(JSON.stringify(["REQ", "sub-since", { kinds: [23194], "#p": [pk], since: now - 300 }]));
        }
      }

      if (msg[0] === "EOSE" && msg[1] === "sub-since") {
        const recentEvent = finalizeEvent({
          kind: 23194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", pk]],
          content: "recent event for since test"
        }, sk);
        ws.send(JSON.stringify(["EVENT", recentEvent]));
      }

      if (msg[0] === "EVENT" && msg[1] === "sub-since") {
        sinceReceived = true;
        console.log("✅ Filter matching on since works.");
        ws.send(JSON.stringify(["CLOSE", "sub-since"]));

        const now2 = Math.floor(Date.now() / 1000);
        ws.send(JSON.stringify(["REQ", "sub-until", { kinds: [23194], "#p": [pk], until: now2 + 3600 }]));
      }

      if (msg[0] === "EOSE" && msg[1] === "sub-until") {
        const untilEvent = finalizeEvent({
          kind: 23194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", pk]],
          content: "event for until test"
        }, sk);
        ws.send(JSON.stringify(["EVENT", untilEvent]));
      }

      if (msg[0] === "EVENT" && msg[1] === "sub-until") {
        untilReceived = true;
        console.log("✅ Filter matching on until works.");
        ws.send(JSON.stringify(["CLOSE", "sub-until"]));
      }

      if (eTagReceived && sinceReceived && untilReceived) {
        ws.close();
        resolve();
      }
    });

    setTimeout(() => {
      reject(new Error(`Filter matching timeout. e-tag: ${eTagReceived}, since: ${sinceReceived}, until: ${untilReceived}`));
    }, 8000);
  });
}

export async function testFilterMatchingAdvanced() {
  console.log("\n--- Testing Advanced Filter Matching (authors, kinds, ids) ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  return new Promise((resolve, reject) => {
    let authorsReceived = false;
    let kindsReceived = false;
    let idsReceived = false;

    ws.on("open", () => {
      ws.send(JSON.stringify(["REQ", "sub-authors", { kinds: [23194], authors: [pk] }]));
    });

    const phase2Kinds = () => {
      ws.send(JSON.stringify(["REQ", "sub-kinds", { kinds: [13194], authors: [pk] }]));
    };

    const phase3Ids = (knownEvent) => {
      ws.send(JSON.stringify(["REQ", "sub-ids", { ids: [knownEvent.id], "#p": [pk] }]));
      setTimeout(() => {
        ws.send(JSON.stringify(["EVENT", knownEvent]));
      }, 200);
    };

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());

      if (!authorsReceived && msg[0] === "EOSE" && msg[1] === "sub-authors") {
        const event = finalizeEvent({
          kind: 23194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [["p", pk]],
          content: "authors test"
        }, sk);
        ws.send(JSON.stringify(["EVENT", event]));
      }

      if (!authorsReceived && msg[0] === "EVENT" && msg[1] === "sub-authors") {
        if (msg[2].pubkey === pk) {
          authorsReceived = true;
          console.log("✅ Filter matching on authors works.");
          ws.send(JSON.stringify(["CLOSE", "sub-authors"]));
          phase2Kinds();
        }
      }

      if (authorsReceived && !kindsReceived && msg[0] === "EOSE" && msg[1] === "sub-kinds") {
        const infoEvent = finalizeEvent({
          kind: 13194,
          created_at: Math.floor(Date.now() / 1000),
          tags: [],
          content: "pay_invoice"
        }, sk);
        ws.send(JSON.stringify(["EVENT", infoEvent]));
        setTimeout(() => {
          ws.send(JSON.stringify(["CLOSE", "sub-kinds"]));
          ws.send(JSON.stringify(["REQ", "sub-kinds2", { kinds: [13194], authors: [pk] }]));
        }, 300);
      }

      if (authorsReceived && !kindsReceived && msg[0] === "EVENT" && (msg[1] === "sub-kinds" || msg[1] === "sub-kinds2")) {
        if (msg[2].kind === 13194) {
          kindsReceived = true;
          console.log("✅ Filter matching on kinds works (info event cached and retrieved).");
          ws.send(JSON.stringify(["CLOSE", msg[1]]));

          const knownEvent = finalizeEvent({
            kind: 23194,
            created_at: Math.floor(Date.now() / 1000),
            tags: [["p", pk]],
            content: "ids test"
          }, sk);
          phase3Ids(knownEvent);
          ws._pendingId = knownEvent.id;
        }
      }

      if (kindsReceived && !idsReceived && msg[0] === "EVENT" && msg[1] === "sub-ids") {
        if (msg[2].id === ws._pendingId) {
          idsReceived = true;
          console.log("✅ Filter matching on ids works.");
          ws.send(JSON.stringify(["CLOSE", "sub-ids"]));
        }
      }

      if (authorsReceived && kindsReceived && idsReceived) {
        ws.close();
        resolve();
      }
    });

    ws.on("error", reject);
    setTimeout(() => {
      reject(new Error(`Advanced filter matching timeout. Authors: ${authorsReceived}, Kinds: ${kindsReceived}, IDs: ${idsReceived}`));
    }, 15000);
  });
}

export async function testMixedValidInvalidFilters() {
  console.log("\n--- Testing Mixed Valid/Invalid Filters in REQ ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  return new Promise((resolve, reject) => {
    let mixedRejected = false;
    let validStillWorks = false;

    ws.on("open", () => {
      ws.send(JSON.stringify(["REQ", "mixed-sub", { kinds: [23194], "#p": [pk] }, { kinds: [23194] }]));
    });

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());

      if (msg[0] === "CLOSED" && msg[1] === "mixed-sub" && msg[2]?.includes("filter too broad")) {
        mixedRejected = true;
        console.log("✅ Mixed valid/invalid REQ rejected entirely (filter too broad).");

        ws.send(JSON.stringify(["REQ", "valid-sub", { kinds: [23194], "#p": [pk] }]));
      }

      if (msg[0] === "EOSE" && msg[1] === "valid-sub") {
        validStillWorks = true;
        console.log("✅ Valid REQ still works after mixed rejection.");
        ws.send(JSON.stringify(["CLOSE", "valid-sub"]));
      }

      if (mixedRejected && validStillWorks) {
        ws.close();
        resolve();
      }
    });

    ws.on("error", reject);
    setTimeout(() => {
      reject(new Error(`Mixed filters test timeout. Mixed rejected: ${mixedRejected}, Valid works: ${validStillWorks}`));
    }, 5000);
  });
}

export async function testFilterLimit() {
  console.log("\n--- Testing Filter Limit (limit: 0 on REQ) ---");

  const sk = generateSecretKey();
  const pk = getPublicKey(sk);
  const ws = new WebSocket(RELAY_URL);

  return new Promise((resolve, reject) => {
    ws.on("open", () => {
      ws.send(JSON.stringify(["REQ", "limit-sub", { kinds: [23194], "#p": [pk], limit: 0 }]));
    });
    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "EOSE" && msg[1] === "limit-sub") {
        console.log("✅ Filter limit=0 returns EOSE (no crash).");
        ws.close();
        resolve();
      }
    });
    ws.on("error", reject);
    setTimeout(() => reject(new Error("Filter limit test timeout")), 5000);
  });
}
