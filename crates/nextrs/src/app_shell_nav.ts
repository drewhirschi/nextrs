// Soft navigation + intent preload for the generated app shell.
//
// Spliced verbatim into the shell entry by bundle.rs (`app_shell_entry`),
// after `router` and `NX_APP_ROUTES` are declared. Not a standalone module.
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
document.addEventListener("click", (e) => {
  if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
  const url = nxAppUrl(e);
  if (!url) return;
  e.preventDefault();
  router.navigate({ href: url.pathname + url.search + url.hash });
});

// ---- Intent preload --------------------------------------------------------
// Plain anchors have no TanStack <Link> machinery, so defaultPreload:"intent"
// never sees them — drive the preload here. preloadRoute warms the
// destination route's lazy CHUNK and runs its loader, and loaders exist only
// on prefetch-backed routes — so /__nx/prefetch is only hit where the server
// actually mounts it.
//
// Every signal funnels through nxPreload, and each URL preloads at most once
// per NX_PRELOAD_TTL whichever signal gets there first. The window matches
// the 30s freshness nxFetchSeeds honours.
const NX_PRELOAD_TTL = 30_000;
const nxPreloaded = new Map<string, number>();
function nxPreload(url: URL, signal: string): boolean {
  const key = url.pathname + url.search;
  // A link to the page we are already on has nothing to warm.
  if (key === window.location.pathname + window.location.search) return false;
  const at = nxPreloaded.get(key);
  if (at !== undefined && performance.now() - at < NX_PRELOAD_TTL) return false;
  nxPreloaded.set(key, performance.now());
  if (nxDebug) console.debug("[nx preload]", signal, key);
  // NOTE: preloadRoute takes NavigateOptions (to/search), NOT { href } —
  // buildLocation ignores href and would preload the current route.
  router
    .preloadRoute({ to: url.pathname, search: Object.fromEntries(url.searchParams) })
    .catch(() => {});
  return true;
}
// Baseline: the pointer is ON a link. Always wired, so preload works before
// the predictor below has loaded and wherever it stands down.
document.addEventListener("mouseover", (e) => {
  const url = nxAppUrl(e);
  if (url) nxPreload(url, "hover");
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
// paint. Every app link is registered automatically — pages keep writing
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
let nxSignal = "foresight";
const nxRegistered = new WeakSet<Element>();
function nxRegisterLinks() {
  if (!nxForesight) return;
  for (const a of document.querySelectorAll<HTMLAnchorElement>("a[href]")) {
    if (nxRegistered.has(a) || !nxAnchorUrl(a)) continue;
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
import("./__nx_foresight__.mjs")
  .then(({ ForesightManager }) => {
    // Defaults throughout, except: leave the app's DOM alone (ForesightJS
    // would otherwise stamp data-predicted/data-status onto every link).
    nxForesight = ForesightManager.initialize({ setDataAttributes: false }) as NxForesight;
    nxRegisterLinks();
    // Pick up links rendered later (soft navigation, lists, late data).
    let queued = false;
    new MutationObserver(() => {
      if (queued) return;
      queued = true;
      requestAnimationFrame(() => {
        queued = false;
        nxRegisterLinks();
      });
    }).observe(document.documentElement, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["href"],
    });
    if (nxDebug) nxDebugAttach();
  })
  .catch(() => {}); // predictor unavailable — hover preload still works

// ---- Debug overlay ---------------------------------------------------------
// Draws the prediction vector (pink = mouse trajectory, blue = scroll),
// flashes a green box around a link the moment its preload is predicted, and
// logs every preload with the signal that fired it. Off by default. From the
// browser console:
//   __nx_preload_debug__()        turn on  (persists in localStorage)
//   __nx_preload_debug__(false)   turn off
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
  nxForesight.addEventListener("callbackInvoked", (e) => {
    nxSignal = e.hitType.subType ? e.hitType.kind + "/" + e.hitType.subType : e.hitType.kind;
    if (nxDebug) nxFlashBox(e.element);
  });
}
Object.assign(window, {
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

