// Cold-start fleet pinger. For each app in fleet.json, fires one concurrent
// burst: `burst` requests at the app's page path + `burst` at its api path,
// all in flight together. The requests that force instances to boot come
// back cold (x-nextrs-cold: 1); the rest ride warm instances — one batch
// measures both temperatures, on both target kinds.
//
//   node metrics/ping.mjs            # NDJSON to stdout, one line per request
//
// Run by .github/workflows/coldstart-pinger.yml, which POSTs the batch to
// the docs site's /api/coldstarts (Turso-backed). No dependencies; node>=20.

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const fleet = JSON.parse(readFileSync(path.join(here, "fleet.json"), "utf8"));
const BURST = fleet.burst ?? 20;

async function one(app, target, targetPath, i) {
  const url = app.url + targetPath;
  const started = performance.now();
  const record = {
    ts: new Date().toISOString(),
    app: app.name,
    target,
    url,
    ok: false,
    status: 0,
    ms: 0,
    temp: "unknown",
    extra: { i, burst: BURST },
  };
  try {
    const res = await fetch(url, {
      redirect: "manual",
      headers: { "user-agent": "nextrs-coldstart-pinger" },
      signal: AbortSignal.timeout(30_000),
    });
    await res.arrayBuffer(); // full body → ms covers the whole response
    record.ms = Math.round(performance.now() - started);
    record.status = res.status;
    record.ok = res.status < 500;
    const cold = res.headers.get("x-nextrs-cold");
    if (cold !== null) {
      record.temp = cold === "1" ? "cold" : "warm";
      record.uptime_ms = Number(res.headers.get("x-nextrs-uptime-ms"));
      record.boot_id = res.headers.get("x-nextrs-boot-id");
    }
  } catch (err) {
    record.ms = Math.round(performance.now() - started);
    record.error = String(err?.cause?.code ?? err?.message ?? err).slice(0, 120);
  }
  return record;
}

const jobs = [];
for (const app of fleet.apps) {
  for (const [target, p] of [["page", app.page], ["api", app.api]]) {
    if (!p) continue;
    for (let i = 0; i < BURST; i++) jobs.push(one(app, target, p, i));
  }
}
const records = await Promise.all(jobs);
for (const r of records) console.log(JSON.stringify(r));
