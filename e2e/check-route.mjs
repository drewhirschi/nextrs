// Shared per-route browser checks used by smoke.mjs (local binaries) and
// prod-smoke.mjs (live deployment). Returns a list of problems; empty = clean.
import { init as initLexer, parse as parseModule } from "es-module-lexer";

// A bare specifier is anything that isn't relative, absolute, or a URL —
// browsers can't resolve those in native ES modules.
export function bareImports(source) {
  try {
    const [imports] = parseModule(source);
    return imports
      .map((i) => i.n)
      .filter((n) => n && !/^(\.|\/|https?:|data:)/.test(n));
  } catch {
    return []; // not parseable as a module — the browser will complain louder
  }
}

export async function checkRoute(browser, base, route) {
  await initLexer;
  const problems = [];
  const context = await browser.newContext();
  const page = await context.newPage();
  page.on("pageerror", (err) => problems.push(`pageerror: ${err.message}`));
  page.on("console", (msg) => {
    if (msg.type() === "error") problems.push(`console.error: ${msg.text()}`);
  });
  page.on("requestfailed", (req) => {
    if (req.url().startsWith(base)) {
      problems.push(`request failed: ${req.url()} (${req.failure()?.errorText})`);
    }
  });
  page.on("response", async (res) => {
    const ct = res.headers()["content-type"] ?? "";
    if (!res.url().startsWith(base) || !ct.includes("javascript")) return;
    if (res.status() >= 400) {
      problems.push(`JS asset ${res.url()} returned ${res.status()}`);
      return;
    }
    const body = await res.text().catch(() => "");
    for (const spec of bareImports(body)) {
      problems.push(`bare import "${spec}" in ${res.url()} — unbundled dependency`);
    }
  });

  // Not "networkidle": dev builds hold a livereload connection open, so the
  // network is never idle. Load + a settle window for the JS to run.
  const res = await page
    .goto(base + route, { waitUntil: "load", timeout: 20000 })
    .catch((err) => {
      problems.push(`navigation failed: ${err.message.split("\n")[0]}`);
      return null;
    });
  if (res && res.status() >= 400) problems.push(`HTTP ${res.status()}`);
  await page.waitForTimeout(750);

  // If the page is a React page (has the mount node), React must mount.
  const mount = await page
    .evaluate(() => {
      const root = document.getElementById("__nx_root__");
      return root ? root.children.length > 0 : null;
    })
    .catch(() => null);
  if (mount === false) problems.push("React mount node #__nx_root__ is empty");

  await context.close();
  return problems;
}
