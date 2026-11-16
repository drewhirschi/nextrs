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
//
// It also covers predictive preload (the vendored ForesightJS): heading for a
// link warms it before the pointer arrives, tabbing toward one warms it ahead
// of focus, and the prediction-vector debug overlay turns on and off.
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

  // --- Predictive preload (vendored ForesightJS) -------------------------
  // A fresh context: nothing cached, no preload dedup carried over.
  //
  //   4. the pointer HEADS for /about and stops short of it → its chunk
  //      downloads anyway (trajectory prediction, not hover)
  //   5. the debug overlay is absent until asked for, then draws the
  //      prediction vector
  //   6. Tab toward the seeded /todos/1 link → its prefetch fires before
  //      focus reaches it
  //   7. prediction leaves the app's DOM alone (no stamped attributes)
  console.log(`\n=== predictive preload on ${base}`);
  const context2 = await browser.newContext({ viewport: { width: 1000, height: 700 } });
  const page2 = await context2.newPage();
  const js2 = [];
  const prefetch2 = [];
  page2.on("request", (req) => {
    const url = req.url();
    if (url.includes("/__nx/prefetch")) prefetch2.push(decodeURIComponent(url));
    else if (url.startsWith(base) && new URL(url).pathname.endsWith(".js")) js2.push(url);
  });
  await page2.goto(base + "/", { waitUntil: "load", timeout: 20000 });
  await page2.waitForSelector('a[href="/todos/1"]', { timeout: 10000 });
  // The predictor is a lazy chunk fetched after the shell boots.
  await page2.waitForFunction(() => performance.getEntriesByType("resource").some((r) => r.name.includes("__nx_foresight__")), null, { timeout: 10000 });
  await page2.waitForTimeout(300);
  check(
    (await page2.locator("svg[data-nx-preload-debug]").count()) === 0,
    "debug overlay is not in the page by default"
  );
  await page2.evaluate(() => window.__nx_preload_debug__());

  // 4. Glide along the header toward /about from its left (nothing else
  //    clickable on that line), ~1.2px/ms, and stop 30px short.
  const about = await page2.locator('a[href="/about"]').boundingBox();
  const y = about.y + about.height / 2;
  const stopX = about.x - 30;
  const jsBefore2 = js2.length;
  let sawVector = false;
  for (let x = about.x - 250; x <= stopX; x += 20) {
    await page2.mouse.move(x, y);
    await page2.waitForTimeout(16);
    sawVector ||= await page2.evaluate(() => {
      const line = document.querySelector("svg[data-nx-preload-debug] line");
      return !!line && line.style.display !== "none" && line.getAttribute("x2") !== line.getAttribute("x1");
    });
  }
  await page2.waitForTimeout(600);
  const overAbout = await page2.evaluate(
    ([px, py]) => !!document.elementFromPoint(px, py)?.closest('a[href="/about"]'),
    [stopX, y]
  );
  check(!overAbout, "the pointer stopped short of /about (never hovered it)");
  check(
    js2.slice(jsBefore2).some((u) => /\/about-/.test(u)),
    `heading for /about preloads its chunk before the pointer arrives (new JS: ${js2.slice(jsBefore2).join(", ") || "none"})`
  );
  // 5.
  check(sawVector, "debug overlay draws the prediction vector while the pointer moves");

  // 6. Park the pointer away from every link, then tab until focus is one
  //    stop BEFORE /todos/1. Tab prediction looks a couple of stops ahead.
  await page2.mouse.move(5, 690);
  await page2.waitForTimeout(300);
  const order = await page2.evaluate(() => {
    const els = [...document.querySelectorAll("a[href], button, input, select, textarea, [tabindex]")].filter(
      (el) => el.tabIndex >= 0 && !el.disabled && el.getClientRects().length > 0
    );
    return els.indexOf(document.querySelector('a[href="/todos/1"]'));
  });
  check(order >= 1, `/todos/1 is reachable by Tab (tab stop #${order})`);
  for (let i = 0; i < order; i++) {
    await page2.keyboard.press("Tab");
    await page2.waitForTimeout(60);
  }
  await page2.waitForTimeout(500);
  const focusedHref = await page2.evaluate(() => document.activeElement?.getAttribute("href"));
  check(focusedHref !== "/todos/1", `focus has not reached /todos/1 yet (focus is on ${focusedHref ?? "a non-link control"})`);
  check(
    prefetch2.filter((u) => u.endsWith("path=/todos/1")).length === 1,
    `tabbing toward seeded /todos/1 prefetches it exactly once, ahead of focus (saw ${prefetch2.join(", ") || "none"})`
  );

  // 7.
  const stamped = await page2.evaluate(
    () => document.querySelectorAll("a[data-predicted], a[data-status], a[data-foresight]").length
  );
  check(stamped === 0, `prediction stamps no attributes onto the app's links (saw ${stamped})`);

  // Turning the overlay off removes it.
  await page2.evaluate(() => window.__nx_preload_debug__(false));
  check(
    (await page2.locator("svg[data-nx-preload-debug]").count()) === 0,
    "__nx_preload_debug__(false) removes the overlay"
  );
  await context2.close();
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
console.log("\nPreload passed: chunks warm on hover and on prediction, /__nx/prefetch only where seeded.");
