// Headless smoke test: boot each app binary and load every route with a real
// browser, failing on anything a user would see as a broken page.
//
// Exists because of docs/postmortems/2026-07-11-docs-site-dead-landing.md:
// a green build shipped a landing page that threw on every load. Cargo tests,
// tsc, and the bundler all passed — only loading the page in a browser fails.
//
// Per route this asserts:
//   - HTTP status < 400
//   - no uncaught page errors (the outage's TypeError lands here)
//   - no console.error output
//   - no failed same-origin subresource requests (missing chunks land here)
//   - no bare import specifiers in served same-origin JS modules
//   - React apps actually mount (#__nx_root__ has children when present)
//
// Usage: node e2e/smoke.mjs [app-name ...]   (default: all apps in apps.json)
// Binaries must already be built: cargo build -p site -p react-todos

import { chromium } from "playwright";
import { checkRoute } from "./check-route.mjs";
import { repoRoot, startApp } from "./app-server.mjs";
import { readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
const config = JSON.parse(readFileSync(path.join(repoRoot, "e2e/apps.json"), "utf8"));
const only = process.argv.slice(2);
const apps = config.apps.filter((a) => only.length === 0 || only.includes(a.name));
if (apps.length === 0) {
  console.error(`no apps matched ${JSON.stringify(only)}`);
  process.exit(2);
}

function discoverRoutes(app) {
  const appDir = path.join(repoRoot, app.appDir);
  const routes = [];
  (function walk(dir, urlPath) {
    const entries = readdirSync(dir);
    if (entries.some((e) => /^page\.(tsx|rs|html)$/.test(e))) {
      routes.push(urlPath || "/");
    }
    for (const e of entries) {
      const full = path.join(dir, e);
      if (!statSync(full).isDirectory()) continue;
      const seg = `${urlPath}/${e}`;
      if (e.startsWith("[")) {
        const sample = app.dynamicSamples?.[seg];
        if (sample) routes.push(sample);
        else console.log(`  (skipping dynamic route ${seg} — no sample in apps.json)`);
        continue;
      }
      walk(full, seg);
    }
  })(appDir, "");
  return routes;
}

async function smokeApp(browser, app) {
  const routes = discoverRoutes(app);
  const { base, child, logTail } = await startApp(app);
  console.log(`\n=== ${app.name} on ${base} — routes: ${routes.join(" ")}`);

  const failures = [];
  try {
    for (const route of routes) {
      const problems = await checkRoute(browser, base, route);
      if (problems.length) {
        failures.push({ route, problems });
        console.log(`  ✗ ${route}`);
        for (const p of problems) console.log(`      ${p}`);
      } else {
        console.log(`  ✓ ${route}`);
      }
    }
  } finally {
    child.kill("SIGTERM");
  }
  if (failures.length) {
    console.log(`  --- ${app.name} server log tail ---`);
    console.log(logTail());
  }
  return failures;
}

const browser = await chromium.launch();
let failed = false;
for (const app of apps) {
  const failures = await smokeApp(browser, app);
  if (failures.length) failed = true;
}
await browser.close();
if (failed) {
  console.error("\nSMOKE FAILED — a user-visible breakage would ship.");
  process.exit(1);
}
console.log("\nSmoke passed: every route loads clean in a real browser.");
