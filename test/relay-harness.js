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

/** How long a relay may take to answer its first request. */
const START_TIMEOUT_MS = 60_000;

/** How long a shut-down relay may keep its port before it is killed. */
const STOP_TIMEOUT_MS = 5_000;

/**
 * PIDs of the processes holding anything on `port`, excluding this one.
 *
 * `lsof` also reports the test process's own keep-alive connection to an
 * earlier relay on the same port, and killing that kills the test.
 */
function portHolders(port) {
  let output = "";
  try {
    output = execSync(`lsof -ti :${port}`, {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    });
  } catch (e) {
    // Either nothing holds the port or the machine has no `lsof`; nothing to
    // kill in both cases.
    return [];
  }
  return [...new Set(output.trim().split(/\s+/).filter(Boolean))]
    .map(Number)
    .filter((pid) => pid !== process.pid);
}

/**
 * Kills whatever still holds `port` and resolves true once it is clear.
 *
 * A relay left behind by an interrupted run keeps the next `wrangler dev` from
 * binding, and wrangler waits on a bound port instead of reporting the
 * conflict, so the port is cleared before every start rather than trusted.
 */
async function freePort(port, timeoutMs = STOP_TIMEOUT_MS) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const holders = portHolders(port);
    if (holders.length === 0) {
      return true;
    }
    for (const pid of holders) {
      try {
        process.kill(pid, "SIGKILL");
      } catch (e) {
        // Already gone.
      }
    }
    await wait(100);
  }
  return portHolders(port).length === 0;
}

/**
 * Starts a private `wrangler dev` on `port` with the given vars, runs `fn`
 * against it, and always stops it again.
 *
 * `persistTo` points the relay at its own state directory; without it wrangler
 * reuses the project's `.wrangler/state`, which other tests and earlier runs
 * also write to. `fn` receives `log`, which returns everything the server has
 * printed so far — the only way to observe work the relay does without a
 * client-visible response.
 */
export async function withRelay(
  { port, vars = {}, secret = "limited-test-secret", persistTo },
  fn,
) {
  const httpURL = `http://127.0.0.1:${port}/${secret}`;
  const wsURL = `ws://127.0.0.1:${port}/${secret}`;

  // Clear the port first: a relay left behind by an interrupted run keeps the
  // new server from binding, and a start that cannot bind hangs rather than
  // fails, which turns into a suite that never finishes.
  if (!(await freePort(port))) {
    throw new Error(`port ${port} is still held by another process`);
  }

  const args = ["wrangler", "dev", "--port", String(port), "--ip", "127.0.0.1"];
  const defaults = { ...QUIET_LIMITS };
  for (const [key, value] of Object.entries({ ...defaults, ...vars })) {
    args.push("--var", `${key}:${value}`);
  }
  args.push("--var", `RELAY_SECRET:${secret}`);
  if (persistTo) {
    args.push("--persist-to", persistTo);
  }

  console.log(`Starting relay on port ${port} (${JSON.stringify(vars)})...`);
  const server = spawn("npx", args, {
    cwd: projectRoot,
    // Its own process group: `wrangler dev` runs as npx → node → workerd, and
    // signalling only the process that was spawned leaves the others behind
    // with the port still bound.
    detached: true,
    stdio: ["ignore", "pipe", "pipe"],
  });

  let output = "";
  server.stdout.on("data", (chunk) => (output += chunk.toString()));
  server.stderr.on("data", (chunk) => (output += chunk.toString()));

  /** Signals the whole tree, falling back to the process if the group is gone. */
  const signalTree = (sig) => {
    try {
      process.kill(-server.pid, sig);
    } catch (e) {
      try {
        server.kill(sig);
      } catch (e2) {
        // Already gone.
      }
    }
  };

  const stop = async () => {
    if (server.exitCode === null && server.signalCode === null) {
      const exited = new Promise((resolve) => server.once("exit", resolve));
      signalTree("SIGTERM");
      const graceful = await Promise.race([exited.then(() => true), wait(2000).then(() => false)]);
      if (!graceful) {
        signalTree("SIGKILL");
        await Promise.race([exited, wait(2000)]);
      }
    }
    // Scenarios reuse ports, and the next start would otherwise bind against a
    // server that is still shutting down.
    await freePort(port);
  };

  try {
    const deadline = Date.now() + START_TIMEOUT_MS;
    let ready = false;
    while (Date.now() < deadline) {
      if (server.exitCode !== null) {
        throw new Error(
          `relay on port ${port} exited early (${server.exitCode}):\n${output.slice(-2000)}`,
        );
      }
      try {
        // Bounded: a server that accepts the connection but never answers must
        // not stall the wait for good.
        const resp = await fetch(httpURL, { signal: AbortSignal.timeout(2000) });
        if (resp.status === 404 || resp.status === 200) {
          ready = true;
          break;
        }
      } catch (e) {
        // Not listening yet.
      }
      await wait(500);
    }
    if (!ready) {
      throw new Error(`relay on port ${port} did not start:\n${output.slice(-2000)}`);
    }
    console.log("Relay is up.");

    await fn({ httpURL, wsURL, log: () => output });
  } finally {
    await stop();
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
