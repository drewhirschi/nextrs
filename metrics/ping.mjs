// Cold-start fleet pinger. Hits every app in fleet.json once and emits one
// NDJSON record per app on stdout. Run by the scheduled workflow
// (.github/workflows/coldstart-pinger.yml), usable by hand:
//
//   node metrics/ping.mjs            # ping the fleet, NDJSON to stdout
//
// Temperature classification (apps with the /__nx/health endpoint):
//   cold  — the instance says this was its first request (x-nextrs-cold: 1),
//           i.e. this ping paid the cold start.
//   warm  — an already-running instance answered.
// Apps without the endpoint get temp: "unknown" — timing only.
//
// No dependencies; node >= 20.

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const fleet = JSON.parse(readFileSync(path.join(here, "fleet.json"), "utf8"));

async function pingOnce(app) {
  const target = app.url + (app.health ? "/__nx/health" : (app.path ?? "/"));
  const started = performance.now();
  const record = {
    ts: new Date().toISOString(),
    app: app.name,
    url: target,
    ok: false,
    status: 0,
    ms: 0,
    temp: "unknown",
  };
  try {
    const res = await fetch(target, {
      redirect: "manual",
      headers: { "user-agent": "nextrs-coldstart-pinger" },
      signal: AbortSignal.timeout(30_000),
    });
    // Read the body so `ms` covers the full response, not just headers.
    const body = await res.text();
    record.ms = Math.round(performance.now() - started);
    record.status = res.status;
    record.ok = res.status < 500;
    if (app.health && res.status === 200) {
      const cold = res.headers.get("x-nextrs-cold");
      const uptime = res.headers.get("x-nextrs-uptime-ms");
      const bootId = res.headers.get("x-nextrs-boot-id");
      if (cold !== null) {
        record.temp = cold === "1" ? "cold" : "warm";
        record.uptime_ms = Number(uptime);
        record.boot_id = bootId;
      } else {
        // App answered but without telemetry headers — not on 0.3.7 yet.
        record.temp = "unknown";
        record.note = body.slice(0, 80);
      }
    }
  } catch (err) {
    record.ms = Math.round(performance.now() - started);
    record.error = String(err?.cause?.code ?? err?.message ?? err).slice(0, 120);
  }
  return record;
}

const records = [];
for (const app of fleet.apps) {
  records.push(await pingOnce(app));
}
for (const r of records) console.log(JSON.stringify(r));
