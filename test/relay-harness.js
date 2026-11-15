import { spawn, execSync } from "child_process";
import { WebSocket } from "ws";

const projectRoot = new URL("..", import.meta.url).pathname.replace(/\/$/, "");

/**
 * Limits switched off by default so each scenario states exactly the gate it
 * exercises; `wrangler.toml` carries the production defaults.
 */
export const QUIET_LIMITS = {
  MAX_CONNECTIONS_PER_IP: 0,
  MAX_CONNECTIONS_PER_ASN: 0,
  IDLE_TIMEOUT_SECONDS: 0,
  CONNECTION_SWEEP_SECONDS: 0,
  EVICT_MIN_IDLE_SECONDS: 3600,
};

export function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Starts a private `wrangler dev` on `port` with the given vars, runs `fn`
 * against it, and always stops it again.
 */
export async function withRelay({ port, vars, secret = "limited-test-secret" }, fn) {
  const httpURL = `http://127.0.0.1:${port}/${secret}`;
  const wsURL = `ws://127.0.0.1:${port}/${secret}`;

  try {
    execSync(`lsof -ti :${port} | xargs kill -9 2>/dev/null || true`);
  } catch (e) {}

  const args = ["wrangler", "dev", "--port", String(port), "--ip", "127.0.0.1"];
  for (const [key, value] of Object.entries({ ...QUIET_LIMITS, ...vars })) {
    args.push("--var", `${key}:${value}`);
  }
  args.push("--var", `RELAY_SECRET:${secret}`);

  console.log(`Starting relay on port ${port} (${JSON.stringify(vars)})...`);
  const server = spawn("npx", args, {
    cwd: projectRoot,
    stdio: ["ignore", "pipe", "pipe"],
  });

  let output = "";
  server.stdout.on("data", (chunk) => (output += chunk.toString()));
  server.stderr.on("data", (chunk) => (output += chunk.toString()));

  const stop = () => {
    try {
      server.kill("SIGTERM");
    } catch (e) {}
    setTimeout(() => {
      try {
        server.kill("SIGKILL");
      } catch (e) {}
    }, 2000);
  };

  try {
    let ready = false;
    for (let i = 0; i < 30; i++) {
      await wait(2000);
      try {
        const resp = await fetch(httpURL);
        if (resp.status === 404 || resp.status === 200) {
          ready = true;
          break;
        }
      } catch (e) {}
    }
    if (!ready) {
      throw new Error(`relay on port ${port} did not start:\n${output.slice(-2000)}`);
    }
    console.log("Relay is up.");

    await fn({ httpURL, wsURL });
  } finally {
    stop();
  }
}

/** Opens a WebSocket and resolves with it once the handshake completes. */
export function connect(url, headers = {}) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url, { headers });
    const timer = setTimeout(() => reject(new Error(`connect timed out: ${url}`)), 5000);
    ws.on("open", () => {
      clearTimeout(timer);
      resolve(ws);
    });
    ws.on("error", (err) => {
      clearTimeout(timer);
      reject(new Error(`connect failed: ${err.message}`));
    });
  });
}

/** Resolves true when the relay refuses the upgrade, false if it opens. */
export function refused(url, headers = {}, ms = 4000) {
  return new Promise((resolve) => {
    const ws = new WebSocket(url, { headers });
    let settled = false;
    const finish = (result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve(result);
    };
    const timer = setTimeout(() => finish(ws.readyState !== WebSocket.OPEN), ms);
    ws.on("open", () => {
      finish(false);
      ws.close();
    });
    ws.on("error", () => finish(true));
    ws.on("close", () => finish(ws.readyState !== WebSocket.OPEN));
  });
}

/**
 * Resolves true once `ws` is closing or closed, false if it is still open
 * after `ms`. Polling the state rather than waiting for the `close` event
 * keeps the check independent of how the local proxy finishes the closing
 * handshake.
 */
export async function closedWithin(ws, ms) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    if (ws.readyState === WebSocket.CLOSING || ws.readyState === WebSocket.CLOSED) {
      return true;
    }
    await wait(100);
  }
  return false;
}
