// Soft navigation + intent preload for the generated app shell.
//
// Spliced verbatim into the shell entry by bundle.rs (`app_shell_entry`),
// after `router`, `NX_APP_ROUTES` and `NX_ROUTE_INFO` (same order: template,
// has-data flag, chunk loader) are declared. Not a standalone module.
//
// Transparent soft navigation: pages and layouts use plain <a> tags (zero
// app-page changes), so intercept same-origin clicks on URLs this router can
// render and turn them into router navigations. Anything else — other
// origins, downloads, modified clicks, new tabs, non-app paths (static files,
// API links, strangler routes) — falls through to a normal document load.
// `data-no-soft` opts an anchor out explicitly.
function nxAnchorUrl(a: Element | null): URL | null {
  if (!(a instanceof HTMLAnchorElement)) return null;
  if (a.target && a.target !== "_self") return null;
  if (a.hasAttribute("download") || a.hasAttribute("data-no-soft")) return null;
  const href = a.getAttribute("href");
  if (!href || href.startsWith("#")) return null;
  const url = new URL(a.href, window.location.href);
  if (url.origin !== window.location.origin) return null;
  if (!NX_APP_ROUTES.some((r) => r.test(url.pathname))) return null;
  return url;
}
function nxAppUrl(e: Event): URL | null {
  return nxAnchorUrl(e.target instanceof Element ? e.target.closest("a") : null);
}
function nxRouteIndex(url: URL): number {
  return NX_APP_ROUTES.findIndex((r) => r.test(url.pathname));
}
document.addEventListener("click", (e) => {
  if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
  const url = nxAppUrl(e);
  if (!url) return;
  e.preventDefault();
  const clicked = performance.now();
  const nav = router.navigate({ href: url.pathname + url.search + url.hash });
  nxMeasure(url, clicked, nav);
});

// ---- Preload ---------------------------------------------------------------
// Plain anchors have no TanStack <Link> machinery, so defaultPreload:"intent"
// never sees them — drive the preload here. Two tiers, by what they cost:
//
//   CHUNK (static, content-hashed, cached forever) — warmed for every app
//     link as soon as it is on screen. Nothing to predict: it is nearly free.
//   DATA (a prefetch.rs run: a function invocation, fresh for 30s) — warmed
//     only on INTENT, through router.preloadRoute, which also runs the
//     route's loader. Loaders exist only on prefetch-backed routes, so
//     /__nx/prefetch is only hit where the server actually mounts it.
//
// Intent signals all funnel through nxPreload, and each URL preloads at most
// once per NX_PRELOAD_TTL whichever gets there first. The window matches the
// 30s freshness nxFetchSeeds honours.
const NX_PRELOAD_TTL = 30_000;
type NxPreloadRecord = {
  started: number; // when the preload began
  trigger: string; // the signal that began it
  hover?: number; // first time the pointer was ON the link
  predicted?: number; // first time a predictor fired for it
  predictor?: string; // ...and which one
  done?: number; // when the preload settled
  promise: Promise<unknown>;
  used?: boolean;
};
const nxPreloads = new Map<string, NxPreloadRecord>();
const nxIsHover = (signal: string) => signal === "hover" || signal === "mouse/hover";
function nxLive(key: string): NxPreloadRecord | undefined {
  const rec = nxPreloads.get(key);
  return rec && performance.now() - rec.started < NX_PRELOAD_TTL ? rec : undefined;
}
function nxPreload(url: URL, signal: string) {
  const key = url.pathname + url.search;
  // A link to the page we are already on has nothing to warm.
  if (key === window.location.pathname + window.location.search) return;
  const now = performance.now();
  let rec = nxLive(key);
  if (!rec) {
    if (nxDebug) console.debug("[nx preload]", signal, key);
    // NOTE: preloadRoute takes NavigateOptions (to/search), NOT { href } —
    // buildLocation ignores href and would preload the current route.
    const promise = router
      .preloadRoute({ to: url.pathname, search: Object.fromEntries(url.searchParams) })
      .catch(() => {});
    const started: NxPreloadRecord = { started: now, trigger: signal, promise };
    promise.then(() => {
      started.done = performance.now();
    });
    nxPreloads.set(key, (rec = started));
    nxCount(nxStats.fired, signal);
    nxCount(nxUnsent.fired, signal);
  }
  // Remember every signal's first sighting, not just the winner's: that is
  // what lets nxMeasure say what hover alone would have achieved.
  if (nxIsHover(signal)) rec.hover ??= now;
  else {
    rec.predicted ??= now;
    rec.predictor ??= signal;
  }
}
// Baseline intent: the pointer is ON a link. Always wired, so preload works
// before the predictor below has loaded and wherever it stands down.
document.addEventListener("mouseover", (e) => {
  const url = nxAppUrl(e);
  if (url) nxPreload(url, "hover");
});

// Chunk tier: warm the route chunk of every app link that scrolls into view.
// Bounded by the number of distinct ROUTES on screen, not links — fifty
// /todos/{id} links are one chunk. Skipped on data-saver / 2g connections.
const nxConnection = (navigator as any).connection;
const nxFrugal = !!nxConnection && (nxConnection.saveData || /2g/.test(nxConnection.effectiveType ?? ""));
const nxChunksWarmed = new Set<number>();
const nxIdle: (fn: () => void) => void =
  (window as any).requestIdleCallback?.bind(window) ?? ((fn: () => void) => setTimeout(fn, 200));
const nxOnScreen = new IntersectionObserver((entries) => {
  for (const entry of entries) {
    if (!entry.isIntersecting) continue;
    nxOnScreen.unobserve(entry.target);
    const url = nxAnchorUrl(entry.target);
    const i = url ? nxRouteIndex(url) : -1;
    if (i < 0 || nxChunksWarmed.has(i)) continue;
    nxChunksWarmed.add(i);
    nxIdle(() => {
      NX_ROUTE_INFO[i].chunk().catch(() => {});
    });
  }
});

// ---- Prediction (ForesightJS) ----------------------------------------------
// The vendored ForesightJS predicts which link the user is about to use and
// fires before they get there:
//   mouse   — the pointer's projected trajectory crosses the link
//   tab     — keyboard focus is a couple of tab stops away
//   scroll  — the link is about to scroll under the pointer
//   touch   — a finger went down on the link (touch has no hover)
// It stands down by itself on data-saver / slower-than-3g connections.
//
// Loaded as its own chunk AFTER the shell boots, so it never delays first
// paint. Every app link is picked up automatically — pages keep writing
// plain <a href>. `data-no-soft` opts a link out, as it does for soft nav.
type NxForesight = {
  register(o: {
    element: Element;
    callback: () => void;
    name?: string;
    reactivateAfter?: number;
  }): unknown;
  addEventListener(type: string, listener: (e: any) => void): void;
  getElementBounds(el: Element): { expandedRect: { top: number; left: number; right: number; bottom: number } } | undefined;
};
let nxForesight: NxForesight | null = null;
let nxSignal = "predicted";
const nxObserved = new WeakSet<Element>();
const nxRegistered = new WeakSet<Element>();
function nxScanLinks() {
  for (const a of document.querySelectorAll<HTMLAnchorElement>("a[href]")) {
    if (!nxAnchorUrl(a)) continue;
    if (!nxFrugal && !nxObserved.has(a)) {
      nxObserved.add(a);
      nxOnScreen.observe(a);
    }
    if (!nxForesight || nxRegistered.has(a)) continue;
    nxRegistered.add(a);
    nxForesight.register({
      element: a,
      name: a.getAttribute("href") ?? "",
      // The link may fire again once its preload has gone stale.
      reactivateAfter: NX_PRELOAD_TTL,
      // Re-read the href at fire time: React reuses anchor nodes.
      callback: () => {
        const url = nxAnchorUrl(a);
        if (url) nxPreload(url, nxSignal);
      },
    });
  }
}
// Pick up links rendered later (first render, soft navigation, late data).
let nxScanQueued = false;
new MutationObserver(() => {
  if (nxScanQueued) return;
  nxScanQueued = true;
  requestAnimationFrame(() => {
    nxScanQueued = false;
    nxScanLinks();
  });
}).observe(document.documentElement, {
  childList: true,
  subtree: true,
  attributes: true,
  attributeFilter: ["href"],
});
import("./__nx_foresight__.mjs")
  .then(({ ForesightManager }) => {
    // Defaults throughout, except: leave the app's DOM alone (ForesightJS
    // would otherwise stamp data-predicted/data-status onto every link).
    const fs = ForesightManager.initialize({ setDataAttributes: false }) as NxForesight;
    // Fires just before a link's callback: says WHICH predictor it was.
    fs.addEventListener("callbackInvoked", (e) => {
      nxSignal = e.hitType.subType ? e.hitType.kind + "/" + e.hitType.subType : e.hitType.kind;
      if (nxDebug) nxFlashBox(e.element);
    });
    nxForesight = fs;
    nxScanLinks();
    if (nxDebug) nxDebugAttach();
  })
  .catch(() => {}); // predictor unavailable — hover preload still works

// ---- Measurement -----------------------------------------------------------
// Is prediction earning its keep in THIS app? For every soft navigation,
// record how long the destination took to load and how much head start each
// strategy had, then work out the wait the user would have felt under each:
//
//   none        no preload at all            wait = load
//   hover       preload when the pointer     wait = load − hover lead
//               lands on the link
//   predictive  hover + the predictors       wait = load − earliest lead
//
// alongside what each strategy COST: preloads fired vs. preloads followed by
// a click. Always collected in memory (it is a few numbers per navigation);
// `__nx_preload_stats__()` in the console prints the comparison.
//
// Shipped to the server only when it asked for it: with
// NEXTRS_PRELOAD_STATS=1 the document carries a <meta name="nx-preload-stats">
// naming the endpoint. Only route TEMPLATES and timings leave the browser —
// never a URL, a param value, or anything about the user.
type NxNav = {
  route: string;
  data: boolean; // the route has a prefetch.rs
  trigger: string; // what started the preload ("none": nothing did)
  load_ms: number; // preload start → ready (or click → ready when none)
  lead_hover_ms: number | null; // head start hover had before the click
  lead_predicted_ms: number | null; // head start the first predictor had
  predictor: string | null;
};
const nxEmptyCounts = () => ({ hover: 0, predicted: 0 });
const nxStats = { navs: [] as NxNav[], fired: nxEmptyCounts(), used: nxEmptyCounts() };
let nxUnsent = { navs: [] as NxNav[], fired: nxEmptyCounts(), used: nxEmptyCounts() };
function nxCount(counts: { hover: number; predicted: number }, signal: string) {
  counts[nxIsHover(signal) ? "hover" : "predicted"]++;
}
const nxRound = (ms: number) => Math.round(ms * 10) / 10;
function nxMeasure(url: URL, clicked: number, nav: Promise<unknown>) {
  const i = nxRouteIndex(url);
  if (i < 0) return;
  const rec = nxLive(url.pathname + url.search);
  if (rec && !rec.used) {
    rec.used = true;
    nxCount(nxStats.used, rec.trigger);
    nxCount(nxUnsent.used, rec.trigger);
  }
  (rec ? rec.promise : nav)
    .catch(() => {})
    .then(() => {
      const entry: NxNav = {
        route: NX_ROUTE_INFO[i].path,
        data: NX_ROUTE_INFO[i].data,
        trigger: rec ? rec.trigger : "none",
        load_ms: nxRound(rec ? (rec.done ?? performance.now()) - rec.started : performance.now() - clicked),
        lead_hover_ms: rec?.hover !== undefined ? nxRound(clicked - rec.hover) : null,
        lead_predicted_ms: rec?.predicted !== undefined ? nxRound(clicked - rec.predicted) : null,
        predictor: rec?.predictor ?? null,
      };
      nxStats.navs.push(entry);
      if (nxStats.navs.length > 500) nxStats.navs.shift();
      if (nxStatsUrl) {
        nxUnsent.navs.push(entry);
        if (nxUnsent.navs.length >= 25) nxShip();
      }
    });
}
// The wait a navigation would have cost under each strategy.
function nxWaits(n: NxNav) {
  const hover = Math.max(0, n.load_ms - (n.lead_hover_ms ?? 0));
  const best = Math.max(n.lead_hover_ms ?? 0, n.lead_predicted_ms ?? 0);
  return { none: n.load_ms, hover, predictive: Math.max(0, n.load_ms - best) };
}
function nxSummary() {
  const navs = nxStats.navs;
  const row = (pick: (n: NxNav) => number) => {
    const waits = navs.map(pick).sort((a, b) => a - b);
    if (!waits.length) return { "median wait (ms)": null, "p90 wait (ms)": null, "instant (%)": null };
    const at = (q: number) => waits[Math.min(waits.length - 1, Math.floor(q * waits.length))];
    return {
      "median wait (ms)": at(0.5),
      "p90 wait (ms)": at(0.9),
      "instant (%)": Math.round((waits.filter((w) => w === 0).length / waits.length) * 100),
    };
  };
  const cost = (k: "hover" | "predicted") => ({
    "preloads fired": nxStats.fired[k],
    "followed by a click": nxStats.used[k],
    "wasted (%)": nxStats.fired[k] ? Math.round((1 - nxStats.used[k] / nxStats.fired[k]) * 100) : null,
  });
  return {
    navigations: navs.length,
    wait: {
      "no preload": row((n) => nxWaits(n).none),
      "hover only": row((n) => nxWaits(n).hover),
      predictive: row((n) => nxWaits(n).predictive),
    },
    // Attributed to whichever signal STARTED each preload.
    cost: { hover: cost("hover"), predicted: cost("predicted") },
    raw: navs,
  };
}
const nxStatsUrl = document.querySelector('meta[name="nx-preload-stats"]')?.getAttribute("content") || null;
function nxShip() {
  if (!nxStatsUrl) return;
  const { navs, fired, used } = nxUnsent;
  if (!navs.length && !fired.hover && !fired.predicted) return;
  nxUnsent = { navs: [], fired: nxEmptyCounts(), used: nxEmptyCounts() };
  try {
    navigator.sendBeacon(nxStatsUrl, JSON.stringify({ navs, fired, used }));
  } catch {}
}
if (nxStatsUrl) {
  // The last moment a page reliably gets to say anything.
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") nxShip();
  });
  window.addEventListener("pagehide", nxShip);
}

// ---- Debug overlay ---------------------------------------------------------
// Draws the prediction vector (pink = mouse trajectory, blue = scroll),
// flashes a green box around a link the moment a predictor fires for it, and
// logs every preload with the signal that started it. Off by default. From
// the browser console:
//   __nx_preload_debug__()        turn on  (persists in localStorage)
//   __nx_preload_debug__(false)   turn off
//   __nx_preload_stats__()        the measured comparison (see above)
const NX_DEBUG_KEY = "nx:preload-debug";
let nxDebug = false;
try {
  nxDebug = localStorage.getItem(NX_DEBUG_KEY) === "1";
} catch {}
function nxSvgEl<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string>) {
  const node = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const k in attrs) node.setAttribute(k, attrs[k]);
  return node;
}
let nxOverlay: { svg: SVGSVGElement; line: SVGLineElement; dot: SVGCircleElement; box: SVGRectElement } | null = null;
function nxOverlayEls() {
  if (!nxOverlay) {
    const svg = nxSvgEl("svg", { "data-nx-preload-debug": "", "aria-hidden": "true" });
    svg.style.cssText =
      "position:fixed;inset:0;width:100vw;height:100vh;pointer-events:none;z-index:2147483647";
    const line = nxSvgEl("line", { "stroke-width": "2", "stroke-linecap": "round" });
    const dot = nxSvgEl("circle", { r: "4" });
    const box = nxSvgEl("rect", { fill: "none", stroke: "#16a34a", "stroke-width": "2", rx: "3" });
    line.style.display = dot.style.display = box.style.display = "none";
    svg.append(box, line, dot);
    document.documentElement.append(svg);
    nxOverlay = { svg, line, dot, box };
  }
  return nxOverlay;
}
let nxVectorTimer = 0;
let nxBoxTimer = 0;
function nxDrawVector(from: { x: number; y: number }, to: { x: number; y: number }, color: string) {
  const { line, dot } = nxOverlayEls();
  line.setAttribute("x1", String(from.x));
  line.setAttribute("y1", String(from.y));
  line.setAttribute("x2", String(to.x));
  line.setAttribute("y2", String(to.y));
  line.setAttribute("stroke", color);
  dot.setAttribute("cx", String(to.x));
  dot.setAttribute("cy", String(to.y));
  dot.setAttribute("fill", color);
  line.style.display = dot.style.display = "";
  // No more updates means the pointer stopped — clear the stale vector.
  clearTimeout(nxVectorTimer);
  nxVectorTimer = window.setTimeout(() => {
    line.style.display = dot.style.display = "none";
  }, 200);
}
function nxFlashBox(el: Element) {
  const r = nxForesight?.getElementBounds(el)?.expandedRect ?? el.getBoundingClientRect();
  const { box } = nxOverlayEls();
  box.setAttribute("x", String(r.left - 2));
  box.setAttribute("y", String(r.top - 2));
  box.setAttribute("width", String(r.right - r.left + 4));
  box.setAttribute("height", String(r.bottom - r.top + 4));
  box.style.display = "";
  clearTimeout(nxBoxTimer);
  nxBoxTimer = window.setTimeout(() => {
    box.style.display = "none";
  }, 900);
}
let nxDebugAttached = false;
function nxDebugAttach() {
  if (nxDebugAttached || !nxForesight) return;
  nxDebugAttached = true;
  // ForesightJS only emits these while something listens, so the overlay
  // costs nothing when it is off.
  nxForesight.addEventListener("mouseTrajectoryUpdate", (e) => {
    if (nxDebug && e.predictionEnabled)
      nxDrawVector(e.trajectoryPositions.currentPoint, e.trajectoryPositions.predictedPoint, "#e11d8f");
  });
  nxForesight.addEventListener("scrollTrajectoryUpdate", (e) => {
    if (nxDebug) nxDrawVector(e.currentPoint, e.predictedPoint, "#2563eb");
  });
}
Object.assign(window, {
  __nx_preload_stats__: () => {
    const summary = nxSummary();
    console.log("[nx preload] " + summary.navigations + " soft navigation(s) this page session");
    console.table(summary.wait);
    console.table(summary.cost);
    return summary;
  },
  __nx_preload_debug__: (on = true) => {
    nxDebug = on;
    try {
      if (on) localStorage.setItem(NX_DEBUG_KEY, "1");
      else localStorage.removeItem(NX_DEBUG_KEY);
    } catch {}
    if (on) nxDebugAttach();
    else {
      nxOverlay?.svg.remove();
      nxOverlay = null;
    }
    return on;
  },
});

