// Post-deploy smoke against a LIVE deployment — the check whose absence let a
// dead docs landing sit in production
// (docs/postmortems/2026-07-11-docs-site-dead-landing.md).
//
// Usage: node e2e/prod-smoke.mjs <base-url> <route> [route ...]
//
// Vercel deploys asynchronously after the push that triggered CI, so a
// failing attempt retries for up to DEADLINE_MS before it counts: right after
// a push, the CDN may still serve the previous (possibly broken) deployment.
// A clean pass at any point within the window means the site users get works.

import { chromium } from "playwright";
import { checkRoute } from "./check-route.mjs";

const [base, ...routes] = process.argv.slice(2);
if (!base || routes.length === 0) {
  console.error("usage: node prod-smoke.mjs <base-url> <route> [route ...]");
  process.exit(2);
}

const DEADLINE_MS = 8 * 60 * 1000;
const RETRY_MS = 30 * 1000;

const browser = await chromium.launch();
const deadline = Date.now() + DEADLINE_MS;
let lastFailures = [];

for (;;) {
  lastFailures = [];
  for (const route of routes) {
    const problems = await checkRoute(browser, base, route);
    if (problems.length) lastFailures.push({ route, problems });
  }
  if (lastFailures.length === 0) {
    console.log(`Prod smoke passed: ${routes.join(" ")} load clean on ${base}`);
    await browser.close();
    process.exit(0);
  }
  const remaining = deadline - Date.now();
  console.log(
    `attempt failed (${lastFailures.length} route(s)); ` +
      (remaining > 0
        ? `retrying in ${RETRY_MS / 1000}s — deploy may still be propagating (${Math.round(remaining / 1000)}s left)`
        : "deadline reached"),
  );
  if (remaining <= 0) break;
  await new Promise((r) => setTimeout(r, RETRY_MS));
}

console.error(`\nPROD SMOKE FAILED on ${base}:`);
for (const { route, problems } of lastFailures) {
  console.error(`  ✗ ${route}`);
  for (const p of problems) console.error(`      ${p}`);
}
await browser.close();
process.exit(1);
