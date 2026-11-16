// Realtime + admin regression test (docs/react-todos-demo-plan.md, steps 1–4).
//
// The demo plan's promises are browser-shaped — "tab B updates", "find the
// 500 tomorrow", "the flaky job retries itself" — so cargo tests alone can't
// hold them. This boots react-todos and asserts, with a real browser:
//
//   1. realtime: two tabs go "● live"; an add in tab A shows in tab B and a
//      toggle in tab B shows in tab A, with no reload
//   2. logs: a todo titled "boom" 500s; after signing in at /__nx/admin, the
//      5xx filter lists it and its record shows the "insert failed" line
//   3. jobs: a "flaky: …" todo's job fails attempt 1, retries itself after
//      its 2s back-off, and succeeds; Retry runs a third attempt
//   4. fail-closed: without NEXTRS_ADMIN_* the portal answers 404
//
// Usage: node e2e/live-admin.mjs   (binary must be built: cargo build -p react-todos)

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
    console.log(`  ✗ ${msg}`);
    failures.push(msg);
  }
}

async function attempt(msg, fn) {
  try {
    await fn();
    check(true, msg);
  } catch (e) {
    check(false, `${msg} — ${e.message.split("\n")[0]}`);
  }
}

const ADD = "input[placeholder='Something to do…']";

process.env.NEXTRS_ADMIN_USER = "e2e";
process.env.NEXTRS_ADMIN_PASSWORD = "e2e-password";
const { base, child, logTail } = await startApp(app);
const browser = await chromium.launch();
console.log(`\n=== react-todos (live + admin) on ${base}`);

try {
  const pageErrors = [];
  const tab = async () => {
    const page = await (await browser.newContext()).newPage();
    page.on("pageerror", (e) => pageErrors.push(e.message));
    await page.goto(`${base}/`);
    return page;
  };

  // 1. realtime between two tabs
  const a = await tab();
  const b = await tab();
  await attempt("both tabs connect (● live)", async () => {
    await a.getByText("● live").waitFor({ timeout: 8000 });
    await b.getByText("● live").waitFor({ timeout: 8000 });
  });
  const title = `from tab A ${Date.now()}`;
  await attempt("an add in tab A appears in tab B without a reload", async () => {
    await a.fill(ADD, title);
    await a.click("button[type=submit]");
    await b.getByText(title).waitFor({ timeout: 5000 });
  });
  await attempt("a toggle in tab B updates tab A", async () => {
    const rowIn = (page) => page.locator("li").filter({ hasText: title });
    const before = await rowIn(a).innerHTML();
    await rowIn(b).locator("input[type=checkbox], button").first().click();
    await a.waitForFunction(
      ([t, html]) => {
        const li = [...document.querySelectorAll("li")].find((l) => l.textContent.includes(t));
        return li && li.innerHTML !== html;
      },
      [title, before],
      { timeout: 5000 },
    );
  });

  // 2. a 500 is findable through the admin portal
  const boom = await fetch(`${base}/api/todos`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ title: "boom" }),
  });
  check(boom.status === 500, `adding "boom" answers 500 (got ${boom.status})`);

  const admin = await (await browser.newContext()).newPage();
  admin.on("pageerror", (e) => pageErrors.push(e.message));
  await attempt("the portal redirects to sign in, then lets the root user in", async () => {
    await admin.goto(`${base}/__nx/admin/logs?status=5xx`);
    await admin.waitForURL(/\/__nx\/admin\/login/);
    await admin.fill("input[name=user]", "e2e");
    await admin.fill("input[name=password]", "e2e-password");
    await admin.click("button[type=submit]");
    await admin.waitForURL(/\/__nx\/admin\/logs\?status=5xx/);
  });
  await attempt("the 5xx filter lists the failed POST and its record has the error line", async () => {
    await admin.locator("tr.link").filter({ hasText: "/api/todos" }).first().click();
    await admin.locator("table.lines").getByText("insert failed").waitFor({ timeout: 3000 });
    await admin.getByText("simulated failure", { exact: false }).first().waitFor({ timeout: 3000 });
  });

  // 3. the flaky job retries itself, then Retry adds an attempt
  const flaky = `flaky: e2e ${Date.now()}`;
  await fetch(`${base}/api/todos`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ title: flaky }),
  });
  const auth = { authorization: `Basic ${btoa("e2e:e2e-password")}` };
  const flakyJob = async () => {
    const res = await fetch(`${base}/__nx/admin/api/jobs?name=audit-todo`, { headers: auth });
    const { jobs } = await res.json();
    return jobs.find((j) => j.payload?.title === flaky);
  };
  let job;
  await attempt("the flaky job fails attempt 1, then succeeds on its own after the back-off", async () => {
    const deadline = Date.now() + 10_000;
    do {
      job = await flakyJob();
      if (job?.status === "succeeded") break;
      await new Promise((r) => setTimeout(r, 250));
    } while (Date.now() < deadline);
    if (job?.status !== "succeeded") throw new Error(`status ${job?.status}`);
    if (job.history.length !== 2 || !job.history[0].error || job.history[1].error) {
      throw new Error(`history ${JSON.stringify(job.history.map((h) => h.error))}`);
    }
    if (job.result?.open_todos === undefined) throw new Error("no stored result");
  });
  await attempt("the job page's Retry button runs a third attempt", async () => {
    await admin.goto(`${base}/__nx/admin/jobs/${job.id}`);
    await admin.getByRole("button", { name: /Retry/ }).click();
    const deadline = Date.now() + 5000;
    let latest;
    do {
      latest = await flakyJob();
      if (latest?.history.length === 3) break;
      await new Promise((r) => setTimeout(r, 200));
    } while (Date.now() < deadline);
    if (latest?.history.length !== 3) throw new Error(`attempts ${latest?.history.length}`);
  });

  check(pageErrors.length === 0, `no page errors${pageErrors.length ? `: ${pageErrors.join(" | ")}` : ""}`);
} finally {
  await browser.close();
  child.kill("SIGTERM");
}

// 4. fail-closed without credentials
delete process.env.NEXTRS_ADMIN_USER;
delete process.env.NEXTRS_ADMIN_PASSWORD;
{
  const { base: bare, child: bareChild } = await startApp(app);
  try {
    const res = await fetch(`${bare}/__nx/admin/logs`, { redirect: "manual" });
    check(res.status === 404, `without NEXTRS_ADMIN_* the portal is 404 (got ${res.status})`);
  } finally {
    bareChild.kill("SIGTERM");
  }
}

if (failures.length) {
  console.log("\n--- react-todos server log tail ---");
  console.log(logTail());
  console.log(`\n${failures.length} live/admin check(s) failed.`);
  process.exit(1);
}
console.log("\nLive + admin checks passed.");
