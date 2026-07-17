// Shared "boot an app binary on a free port" helpers for the e2e scripts
// (smoke.mjs, hover.mjs). Binaries must already be built:
// cargo build -p site -p react-todos
import { spawn } from "node:child_process";
import { createServer } from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export function freePort() {
  return new Promise((resolve, reject) => {
    const srv = createServer();
    srv.listen(0, () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
    srv.on("error", reject);
  });
}

export async function waitForServer(url, child, timeoutMs = 15000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`server exited early (code ${child.exitCode})`);
    try {
      await fetch(url, { signal: AbortSignal.timeout(1000) });
      return;
    } catch {
      await new Promise((r) => setTimeout(r, 150));
    }
  }
  throw new Error(`server at ${url} not up after ${timeoutMs}ms`);
}

// Spawns the app's debug binary on a free port and waits until it accepts
// connections. Returns { base, child, logTail } — caller must child.kill().
export async function startApp(app) {
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const bin = path.join(repoRoot, "target/debug", app.binary);
  const child = spawn(bin, {
    cwd: path.join(repoRoot, app.appDir, ".."),
    env: { ...process.env, PORT: String(port) },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let serverLog = "";
  child.stdout.on("data", (d) => (serverLog += d));
  child.stderr.on("data", (d) => (serverLog += d));
  await waitForServer(base, child);
  return { base, child, logTail: (n = 15) => serverLog.split("\n").slice(-n).join("\n") };
}
