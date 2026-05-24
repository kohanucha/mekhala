import { WebSocket } from "ws";
import { RELAY_URL } from "./env.js";
import { finalizeEvent, generateSecretKey, getPublicKey } from "nostr-tools/pure";

export async function testMalformedEventProducesOK() {
  console.log("\n--- Testing Malformed Event Produces OK False ---");
  const ws = new WebSocket(RELAY_URL);

  return new Promise((resolve, reject) => {
    const eventId = "deadbeef".repeat(8);
    ws.on("open", () => {
      ws.send(JSON.stringify(["EVENT", { id: eventId, content: 123 }]));
    });

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[1] === eventId && msg[2] === false) {
        console.log("✅ Malformed event produced OK false with ID.");
        ws.close();
        resolve();
      }
    });

    setTimeout(() => {
      ws.close();
      reject(new Error("Timeout waiting for OK response for malformed event"));
    }, 5000);
  });
}

export async function testOversizedEventProducesOK() {
  console.log("\n--- Testing Oversized Event Produces OK False ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();

  return new Promise((resolve, reject) => {
    ws.on("open", () => {
      const largeEvent = finalizeEvent({
        kind: 23194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [],
        content: "a".repeat(70000)
      }, sk);

      ws.send(JSON.stringify(["EVENT", largeEvent]));
    });

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === false && msg[3].includes("content too large")) {
        console.log("✅ Oversized event produced OK false with limit error.");
        ws.close();
        resolve();
      }
    });

    setTimeout(() => {
      ws.close();
      reject(new Error("Timeout waiting for OK response for oversized event"));
    }, 5000);
  });
}

export async function testTimestampValidation() {
  console.log("\n--- Testing Timestamp Validation Limits ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  return new Promise((resolve, reject) => {
    ws.on("open", () => {
      console.log("Step 1: Testing rejection of future timestamp...");
      const futureEvent = finalizeEvent({
        kind: 23194,
        created_at: Math.floor(Date.now() / 1000) + 1200,
        tags: [["p", pk]],
        content: "future"
      }, sk);
      ws.send(JSON.stringify(["EVENT", futureEvent]));

      console.log("Step 2: Testing rejection of ancient timestamp...");
      const oldEvent = finalizeEvent({
        kind: 23194,
        created_at: Math.floor(Date.now() / 1000) - (2 * 365 * 24 * 60 * 60),
        tags: [["p", pk]],
        content: "ancient"
      }, sk);
      ws.send(JSON.stringify(["EVENT", oldEvent]));
    });

    let futureRejected = false;
    let oldRejected = false;

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "OK" && msg[2] === false && msg[3].includes("invalid: event creation date")) {
        if (msg[3].includes("far off")) futureRejected = true;
        if (msg[3].includes("too old")) oldRejected = true;
      }

      if (futureRejected && oldRejected) {
        console.log("✅ Timestamp limits enforced correctly.");
        ws.close();
        resolve();
      }
    });

    setTimeout(() => {
      reject(new Error(`Timestamp Validation Timeout. Future: ${futureRejected}, Old: ${oldRejected}`));
    }, 5000);
  });
}

export async function testProtocolErrors() {
  console.log("\n--- Testing Protocol Error Handling ---");
  const ws = new WebSocket(RELAY_URL);

  return new Promise((resolve, reject) => {
    const results = {
      binaryRejected: false,
      unknownTypeRejected: false,
      emptyArrayRejected: false,
      insufficientReqArgsRejected: false,
    };
    let noticeCount = 0;

    ws.on("open", () => {
      ws.send(Buffer.from("binary data"));
      ws.send(JSON.stringify(["UNKNOWN", "test"]));
      ws.send("[]");
      ws.send(JSON.stringify(["REQ"]));
    });

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg[0] === "NOTICE") {
        const reason = msg[1] || "";
        noticeCount++;
        if (reason.includes("binary not supported")) {
          results.binaryRejected = true;
          console.log("✅ Binary message rejected with NOTICE.");
        } else if (reason.includes("empty message")) {
          results.emptyArrayRejected = true;
          console.log("✅ Empty array rejected.");
        } else if (reason.includes("unknown message type")) {
          if (!results.unknownTypeRejected) {
            results.unknownTypeRejected = true;
            console.log("✅ Unknown message type rejected.");
          } else {
            results.insufficientReqArgsRejected = true;
            console.log("✅ Insufficient REQ args rejected (parsed as unknown type).");
          }
        } else if (reason.includes("parse failed")) {
          if (!results.emptyArrayRejected) {
            results.emptyArrayRejected = true;
            console.log("✅ Parse error rejected (general).");
          }
        }
      }

      if (results.binaryRejected && results.unknownTypeRejected &&
          results.emptyArrayRejected && results.insufficientReqArgsRejected) {
        console.log("✅ All protocol errors handled correctly.");
        ws.close();
        resolve();
      }
    });

    ws.on("error", () => {
      if (!results.binaryRejected) {
        results.binaryRejected = true;
        console.log("✅ Binary message caused WebSocket error (expected).");
      }
    });

    setTimeout(() => {
      const status = Object.entries(results).map(([k, v]) => `${k}: ${v}`).join(", ");
      reject(new Error(`Protocol error test timeout. ${status}`));
    }, 5000);
  });
}

export async function testLimitEnforcement() {
  console.log("\n--- Testing Limit Enforcement ---");
  const ws = new WebSocket(RELAY_URL);
  const sk = generateSecretKey();
  const pk = getPublicKey(sk);

  return new Promise((resolve, reject) => {
    let contentLimitRejected = false;

    ws.on("open", () => {
      const bigContent = "a".repeat(65537);
      const eventTooLarge = finalizeEvent({
        kind: 23194,
        created_at: Math.floor(Date.now() / 1000),
        tags: [["p", pk]],
        content: bigContent
      }, sk);
      ws.send(JSON.stringify(["EVENT", eventTooLarge]));
    });

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());

      if (msg[0] === "OK" && msg[2] === false) {
        if (msg[3].includes("content too large")) {
          contentLimitRejected = true;
          console.log("✅ Event with >64KB content rejected.");
        }
      }

      if (contentLimitRejected) {
        console.log("✅ All limit enforcement tests passed.");
        ws.close();
        resolve();
      }
    });

    setTimeout(() => {
      ws.close();
      reject(new Error(`Limit enforcement timeout. Content: ${contentLimitRejected}`));
    }, 8000);
  });
}
