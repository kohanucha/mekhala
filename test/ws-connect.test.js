import { WebSocket } from "ws";
import { RELAY_URL } from "./env.js";

export async function testWebSocketMessageDelivery() {
  console.log("Testing WebSocket message delivery (websocket_message handler)...");
  const ws = new WebSocket(RELAY_URL);

  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      ws.close();
      reject(new Error(
        "No response received: websocket_message handler is NOT being called. "
        + "The DO accepts the WebSocket upgrade but the runtime never delivers messages to the handler."
      ));
    }, 3000);

    ws.on("open", () => {
      console.log("  Connected, sending REQ...");
      ws.send(JSON.stringify(["REQ", "test-ws", { kinds: [1] }]));
    });

    ws.on("message", (data) => {
      clearTimeout(timeout);
      console.log("  Response received:", data.toString().substring(0, 120));
      console.log("✅ websocket_message handler is delivering messages.");
      ws.close();
      resolve();
    });

    ws.on("error", (err) => {
      clearTimeout(timeout);
      reject(err);
    });
  });
}
