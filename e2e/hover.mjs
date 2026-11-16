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
//   2. the UNSEEDED /about link is merely ON SCREEN → its JS chunk downloads
//      (chunks are warmed for every visible link); hovering it then fires
//      no /__nx/prefetch request
//   3. hover over the SEEDED /todos/1 link → exactly one
//      /__nx/prefetch?path=%2Ftodos%2F1 (and it succeeds); hovering it a
//      second time still yields exactly one (dedup)
//
//
// It also covers predictive DATA preload (the vendored ForesightJS): heading
// for a seeded link prefetches it before the pointer arrives, tabbing toward
// one prefetches it ahead of focus, and the prediction-vector debug overlay
// turns on and off. And it covers the measurement: __nx_preload_stats__()
// compares hover against prediction for a real click, and with
// NEXTRS_PRELOAD_STATS=1 the same numbers reach the server as tracing events.
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

  // 2. Unseeded route: its chunk is already here — it was warmed because
  //    the link is on screen, before any pointer movement. Hovering it must
  //    not touch /__nx/prefetch.
  check(
    jsReqs.some((u) => /\/about-/.test(u)),
    `the on-screen /about link's JS chunk is preloaded without any hover (JS so far: ${jsReqs.length})`
  );
  await page.hover('a[href="/about"]');
  await page.waitForTimeout(750);
  check(
    prefetchReqs.length === 0,
    `hovering unseeded /about fires no /__nx/prefetch (saw ${prefetchReqs.join(", ") || "none"})`
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

  // --- Predictive data preload (vendored ForesightJS) --------------------
  // A fresh context: nothing cached, no preload dedup carried over.
  //
  //   4. the pointer HEADS for seeded /todos/3 and stops short of it → its
  //      /__nx/prefetch fires anyway (trajectory prediction, not hover)
  //   5. the debug overlay is absent until asked for, then draws the
  //      prediction vector
  //   6. Tab toward the seeded /todos/1 link → its prefetch fires before
  //      focus reaches it
  //   7. prediction leaves the app's DOM alone (no stamped attributes)
  //   8. finish the move and click /todos/3 → __nx_preload_stats__() shows
  //      prediction had a longer head start than hover; nothing is shipped,
  //      because the server did not ask for it
  console.log(`\n=== predictive preload on ${base}`);
  const context2 = await browser.newContext({ viewport: { width: 1000, height: 700 } });
  const page2 = await context2.newPage();
  const js2 = [];
  const prefetch2 = [];
  const statsPosts2 = [];
  page2.on("request", (req) => {
    const url = req.url();
    if (url.includes("/__nx/preload-stats")) statsPosts2.push(url);
    else if (url.includes("/__nx/prefetch")) prefetch2.push(decodeURIComponent(url));
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

  // 4. Glide toward /todos/3 from the empty margin on its right (nothing
  //    else clickable on that line), ~1.2px/ms, and stop 30px short.
  const todo3 = await page2.locator('a[href="/todos/3"]').boundingBox();
  const y = todo3.y + todo3.height / 2;
  const stopX = todo3.x + todo3.width + 30;
  let sawVector = false;
  for (let x = stopX + 240; x >= stopX; x -= 20) {
    await page2.mouse.move(x, y);
    await page2.waitForTimeout(16);
    sawVector ||= await page2.evaluate(() => {
      const line = document.querySelector("svg[data-nx-preload-debug] line");
      return !!line && line.style.display !== "none" && line.getAttribute("x2") !== line.getAttribute("x1");
    });
  }
  await page2.waitForTimeout(600);
  const overTodo3 = await page2.evaluate(
    ([px, py]) => !!document.elementFromPoint(px, py)?.closest('a[href="/todos/3"]'),
    [stopX, y]
  );
  check(!overTodo3, "the pointer stopped short of /todos/3 (never hovered it)");
  check(
    prefetch2.filter((u) => u.endsWith("path=/todos/3")).length === 1,
    `heading for seeded /todos/3 prefetches it before the pointer arrives (saw ${prefetch2.join(", ") || "none"})`
  );
  // 5.
  check(sawVector, "debug overlay draws the prediction vector while the pointer moves");

  // 6. Park the pointer away from every link, then tab until focus is one
  //    stop BEFORE /todos/1. Tab prediction looks a couple of stops ahead.
  await page2.mouse.move(stopX, 690);
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

  // 8. Come back, land on /todos/3 and click it. Hover only starts now —
  //    seconds after the prediction — so prediction's lead must be longer.
  await page2.mouse.move(stopX, y);
  await page2.mouse.move(todo3.x + todo3.width - 40, y, { steps: 4 });
  await page2.waitForTimeout(120);
  await page2.mouse.down();
  await page2.mouse.up();
  await page2.waitForURL("**/todos/3", { timeout: 10000 });
  await page2.waitForTimeout(300);
  const stats = await page2.evaluate(() => window.__nx_preload_stats__());
  const nav = stats.raw[0];
  check(stats.navigations === 1, `stats recorded the navigation (saw ${stats.navigations})`);
  check(
    nav?.route === "/todos/{id}" && nav.data === true,
    `stats report the route TEMPLATE, not the URL (saw ${JSON.stringify(nav?.route)})`
  );
  check(
    nav?.trigger === "mouse/trajectory" && nav.lead_predicted_ms > nav.lead_hover_ms && nav.lead_hover_ms > 0,
    `prediction had a longer head start than hover (trigger ${nav?.trigger}, predicted ${nav?.lead_predicted_ms}ms vs hover ${nav?.lead_hover_ms}ms)`
  );
  check(
    stats.wait.predictive["median wait (ms)"] <= stats.wait["hover only"]["median wait (ms)"] &&
      stats.wait["hover only"]["median wait (ms)"] <= stats.wait["no preload"]["median wait (ms)"],
    `estimated wait orders predictive ≤ hover ≤ none (${JSON.stringify(stats.wait)})`
  );
  check(stats.cost.predicted["preloads fired"] >= 1, `stats count what prediction cost (${JSON.stringify(stats.cost)})`);
  check(
    (await page2.locator('meta[name="nx-preload-stats"]').count()) === 0 && statsPosts2.length === 0,
    `nothing is shipped when the server has not opted in (POSTs: ${statsPosts2.length})`
  );
  const closed = await page2.request.post(base + "/__nx/preload-stats", { data: "{}" });
  check(closed.status() >= 400, `the stats endpoint is not mounted by default (status ${closed.status()})`);
  await context2.close();
} finally {
  await browser.close();
  child.kill("SIGTERM");
}

// --- Shipping the stats (NEXTRS_PRELOAD_STATS=1) --------------------------
//   9. the document carries the meta tag, the shell POSTs its report when
//      the page is hidden, the server accepts it and logs tracing events
console.log("\n=== preload stats shipping (NEXTRS_PRELOAD_STATS=1)");
process.env.NEXTRS_PRELOAD_STATS = "1";
const optedIn = await startApp(app);
delete process.env.NEXTRS_PRELOAD_STATS;
const browser3 = await chromium.launch();
try {
  const page3 = await browser3.newPage();
  const posts = [];
  page3.on("response", (res) => {
    if (res.url().includes("/__nx/preload-stats")) posts.push({ status: res.status(), body: res.request().postData() });
  });
  await page3.goto(optedIn.base + "/", { waitUntil: "load", timeout: 20000 });
  await page3.waitForSelector('a[href="/todos/1"]', { timeout: 10000 });
  check(
    (await page3.locator('meta[name="nx-preload-stats"]').getAttribute("content")) === "/__nx/preload-stats",
    "the document carries the nx-preload-stats meta tag"
  );
  await page3.hover('a[href="/todos/1"]');
  await page3.waitForTimeout(250);
  await page3.click('a[href="/todos/1"]');
  await page3.waitForURL("**/todos/1", { timeout: 10000 });
  await page3.waitForTimeout(300);
  // The shell reports when the page is hidden (tab switch, close, navigate away).
  await page3.evaluate(() => {
    Object.defineProperty(document, "visibilityState", { value: "hidden", configurable: true });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await page3.waitForTimeout(700);
  check(posts.length === 1 && posts[0].status === 204, `one report POSTed and accepted (saw ${JSON.stringify(posts.map((p) => p.status))})`);
  const sent = posts[0]?.body ? JSON.parse(posts[0].body) : null;
  check(
    sent?.navs?.length === 1 && sent.navs[0].route === "/todos/{id}" && sent.navs[0].lead_hover_ms > 0,
    `the report holds the navigation with its hover lead (${posts[0]?.body})`
  );
  check(
    !/\/todos\/1/.test(posts[0]?.body ?? ""),
    "the report contains no concrete URL"
  );
  const log = optedIn.logTail(400).replace(/\x1b\[[0-9;]*m/g, ""); // strip terminal colors
  check(
    /nextrs::preload.*preload navigation/.test(log) && /http\.route=\/todos\/\{id\}/.test(log) && /wait_hover_ms=/.test(log),
    "the server logged a `preload navigation` tracing event with the derived waits"
  );
  check(/nextrs::preload.*preload cost/.test(log) && /hover_fired=1/.test(log), "the server logged the `preload cost` event");
  const junk = await page3.request.post(optedIn.base + "/__nx/preload-stats", {
    data: JSON.stringify({ navs: [{ route: "/todos/1?token=abc", trigger: "hover", load_ms: 1 }] }),
  });
  check(junk.status() === 400, `a report carrying a real URL is rejected (status ${junk.status()})`);
  if (failures.length) {
    console.log("  --- opted-in server log tail ---");
    console.log(optedIn.logTail(30));
  }
} finally {
  await browser3.close();
  optedIn.child.kill("SIGTERM");
}

if (failures.length) {
  console.error("\nHOVER PRELOAD FAILED:");
  for (const f of failures) console.error(`  - ${f}`);
  console.log("  --- server log tail ---");
  console.log(logTail());
  process.exit(1);
}
console.log("\nPreload passed: chunks warm on hover and on prediction, /__nx/prefetch only where seeded.");
