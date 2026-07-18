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
const BATCH_ID = process.env.GITHUB_RUN_ID
  ? `${process.env.GITHUB_RUN_ID}-${process.env.GITHUB_RUN_ATTEMPT ?? "1"}`
  : crypto.randomUUID();

async function one(app, target, targetPath, i, phase) {
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
    phase,
    extra: {
      i,
      burst: BURST,
      telemetry_version: 2,
      batch_id: BATCH_ID,
      pair: app.pair ?? null,
      expected_region: app.expected_region ?? null,
      runner_name: process.env.RUNNER_NAME ?? null,
      runner_os: process.env.RUNNER_OS ?? process.platform,
      runner_arch: process.env.RUNNER_ARCH ?? process.arch,
      git_sha: process.env.GITHUB_SHA ?? null,
    },
  };
  try {
    const res = await fetch(url, {
      redirect: "manual",
      headers: { "user-agent": "nextrs-coldstart-pinger" },
      signal: AbortSignal.timeout(30_000),
    });
    const body = await res.arrayBuffer(); // full body → ms covers the whole response
    record.ms = Math.round(performance.now() - started);
    record.status = res.status;
    record.ok = res.status < 500;
    const vercelId = res.headers.get("x-vercel-id");
    const route = vercelId?.split("::") ?? [];
    record.extra.vercel_id = vercelId;
    record.extra.edge_region = route[0] || null;
    // Function responses are edge::function::request-id. CDN responses omit
    // the middle function-region component.
    record.extra.function_region = route.length >= 3 ? route[1] || null : null;
    record.extra.region_match =
      record.extra.function_region !== null &&
      record.extra.function_region === record.extra.expected_region;
    record.extra.vercel_cache = res.headers.get("x-vercel-cache");
    record.extra.response_bytes = body.byteLength;
    const cold = res.headers.get("x-nextrs-cold");
    if (cold !== null) {
      record.temp = cold === "1" ? "cold" : "warm";
      const uptime = res.headers.get("x-nextrs-uptime-ms");
      if (uptime !== null) record.uptime_ms = Number(uptime);
      record.boot_id = res.headers.get("x-nextrs-boot-id");
    } else if (target === "page") {
      // Next.js page functions cannot set per-request response headers from a
      // Server Component. Instrumented comparator pages instead render one
      // hidden marker into their HTML, which carries the same process facts.
      const html = new TextDecoder().decode(body);
      const marker = html.match(
        /data-nextrs-cold="([01])"[^>]*data-nextrs-uptime-ms="(\d+)"[^>]*data-nextrs-boot-id="([^"]+)"/,
      );
      if (marker) {
        record.temp = marker[1] === "1" ? "cold" : "warm";
        record.uptime_ms = Number(marker[2]);
        record.boot_id = marker[3];
      }
    }
  } catch (err) {
    record.ms = Math.round(performance.now() - started);
    record.error = String(err?.cause?.code ?? err?.message ?? err).slice(0, 120);
  }
  return record;
}

// Phase 1 — concurrent burst: forces scale-out, catches the cold starts it
// causes. Latencies here include per-socket TLS + queuing: spike numbers.
const jobs = [];
for (const app of fleet.apps) {
  for (const [target, p] of [["page", app.page], ["api", app.api]]) {
    if (!p) continue;
    for (let i = 0; i < BURST; i++) jobs.push(one(app, target, p, i, "burst"));
  }
}
const records = await Promise.all(jobs);

// Phase 2 — one request per target per round. Each individual target remains
// sequential, but all apps are sampled side-by-side so runner/network drift
// cannot become a fake framework difference. The burst just heated them all.
const SEQ = 5;
for (let i = 0; i < SEQ; i++) {
  const round = [];
  for (const app of fleet.apps) {
    for (const [target, p] of [["page", app.page], ["api", app.api]]) {
      if (p) round.push(one(app, target, p, i, "seq"));
    }
  }
  records.push(...(await Promise.all(round)));
}

// A region mismatch invalidates both sides of that pair for this batch. This
// prevents the correctly placed app from accumulating an older comparison
// window while its counterpart is still in the wrong region.
for (const pair of new Set(fleet.apps.map((app) => app.pair).filter(Boolean))) {
  const apps = fleet.apps.filter((app) => app.pair === pair);
  for (const target of ["page", "api"]) {
    const paired = records.filter((record) =>
      apps.some((app) => app.name === record.app) && record.target === target
    );
    const expected = new Set(apps.map((app) => app.expected_region).filter(Boolean));
    const actual = new Set(
      paired.map((record) => record.extra.function_region).filter(Boolean),
    );
    const valid =
      expected.size === 1 && [...actual].every((region) => expected.has(region));
    for (const record of paired) record.extra.pair_region_match = valid;
  }
}
for (const r of records) console.log(JSON.stringify(r));
