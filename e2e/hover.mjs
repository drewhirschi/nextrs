// Hover-preload regression test (docs/upstream-plans/hover-preload-route.md).
//
// The generated app shell used to fire GET /__nx/prefetch on EVERY link
// hover — 404ing for routes without a prefetch.rs — and never preloaded the
// destination route's JS chunk (defaultPreload:"intent" needs <Link>, which
// nextrs's plain anchors bypass). The smoke only loads pages; nothing in CI
// hovered, so both shipped invisibly. This boots react-todos (a mixed app:
// seeded /, /todos/[id]; unseeded /about) and asserts, with a real browser:
//
//   1. page load fires no /__nx/prefetch (first-load skip: seeds streamed)
//   2. hover over the UNSEEDED /about link → no /__nx/prefetch request,
//      but the about page's JS chunk DOES download (chunk preload works)
//   3. hover over the SEEDED /todos/1 link → exactly one
//      /__nx/prefetch?path=%2Ftodos%2F1 (and it succeeds); hovering it a
//      second time still yields exactly one (dedup)
//
// Usage: node e2e/hover.mjs   (binary must be built: cargo build -p react-todos)

import { chromium } from "playwright";
import { startApp } from "./app-server.mjs";

const app = {
  name: "react-todos",
  binary: "react-todos",
  appDir: "examples/react-todos/app",
};

const failures = [];
function check(cond, msg) {
  if (cond) console.log(`  ✓ ${msg}`);
  else {
    failures.push(msg);
    console.log(`  ✗ ${msg}`);
  }
}

const { base, child, logTail } = await startApp(app);
const browser = await chromium.launch();
try {
  const context = await browser.newContext();
  const page = await context.newPage();

  const prefetchReqs = [];
  const jsReqs = [];
  page.on("request", (req) => {
    const url = req.url();
    if (!url.startsWith(base)) return;
    if (url.includes("/__nx/prefetch")) prefetchReqs.push(url);
    else if (new URL(url).pathname.endsWith(".js")) jsReqs.push(url);
  });
  const prefetchResponses = [];
  page.on("response", (res) => {
    if (res.url().includes("/__nx/prefetch")) prefetchResponses.push(res.status());
  });

  console.log(`\n=== hover preload on ${base}`);
  await page.goto(base + "/", { waitUntil: "load", timeout: 20000 });
  await page.waitForSelector('a[href="/todos/1"]', { timeout: 10000 });
  await page.waitForTimeout(500);

  // 1. The hard load already streamed its seeds — the root loader's
  //    first-load skip must not re-request them.
  check(prefetchReqs.length === 0, `page load fires no /__nx/prefetch (saw ${prefetchReqs.length})`);

  // 2. Unseeded route: hover must not touch /__nx/prefetch, but must
  //    download the destination page's chunk.
  const jsBefore = jsReqs.length;
  await page.hover('a[href="/about"]');
  await page.waitForTimeout(750);
  check(
    prefetchReqs.length === 0,
    `hovering unseeded /about fires no /__nx/prefetch (saw ${prefetchReqs.join(", ") || "none"})`
  );
  check(
    jsReqs.length > jsBefore,
    `hovering unseeded /about preloads its JS chunk (new JS requests: ${jsReqs.slice(jsBefore).join(", ") || "none"})`
  );

  // 3. Seeded route: exactly one prefetch request, deduped across hovers.
  await page.hover('a[href="/todos/1"]');
  await page.waitForTimeout(500);
  await page.hover("h1"); // move off the link...
  await page.hover('a[href="/todos/1"]'); // ...and back on
  await page.waitForTimeout(500);
  const todoPrefetches = prefetchReqs.filter((u) => decodeURIComponent(u).includes("/todos/1"));
  check(
    todoPrefetches.length === 1,
    `hovering seeded /todos/1 twice fires exactly one /__nx/prefetch (saw ${todoPrefetches.length}: ${todoPrefetches.join(", ")})`
  );
  check(
    prefetchResponses.length > 0 && prefetchResponses.every((s) => s === 200),
    `every /__nx/prefetch response is 200 (saw ${prefetchResponses.join(", ") || "none"})`
  );
  check(
    prefetchReqs.length === todoPrefetches.length,
    `no stray /__nx/prefetch requests (saw ${prefetchReqs.join(", ")})`
  );

  await context.close();
} finally {
  await browser.close();
  child.kill("SIGTERM");
}

if (failures.length) {
  console.error("\nHOVER PRELOAD FAILED:");
  for (const f of failures) console.error(`  - ${f}`);
  console.log("  --- server log tail ---");
  console.log(logTail());
  process.exit(1);
}
console.log("\nHover preload passed: chunks warm on hover, /__nx/prefetch only where seeded.");
