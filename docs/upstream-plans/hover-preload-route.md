# Hover Preload Should Go Through the Router, Not Straight to /__nx/prefetch

- **Reported-in:** hand-assembled nextrs port (agent session)
- **Date:** 2026-07-16
- **Status:** open   <!-- open | fixed in <commit> | wontfix -->

## Problem

The generated app shell's hover handler and its router config contradict each
other, producing two bugs:

**A — data prefetch 404s.** The `mouseover` handler calls
`nxPrefetch(url.pathname + url.search)` — a direct
`GET /__nx/prefetch?path=...` — for EVERY internal route. But the server only
mounts `/__nx/prefetch` when at least one route has a `prefetch.rs`
(`router.rs`), and the inner dispatcher (`build_prefetch_endpoint`) only
mirrors prefetch-capable routes. An app with zero `prefetch.rs` 404s on every
link hover; a mixed app 404s on hovers over unseeded routes. The client
swallows the failures, but logs, the network tab, and any request metrics fill
with 404s.

**B — chunk preload never happens.** `defaultPreload: "intent"` only works
through TanStack's `<Link>` component. nextrs uses plain `<a>` tags plus a
click interceptor, so TanStack never sees hover intent and the destination
route's JS chunk is NOT preloaded on hover — it downloads at click time. The
shell's closing comment ("the real 'load the next page's React ahead of time'
is TanStack's defaultPreload:'intent'") documents behavior that never fires.

**C — test gap.** The e2e smoke (`e2e/smoke.mjs` + `e2e/check-route.mjs`)
fails on broken network requests but only during page load — nothing in CI
ever hovers a link, so both bugs shipped invisibly.

## Proposed Direction

Route hover intent through the router: the `mouseover` handler calls
`router.preloadRoute(...)` instead of `nxPrefetch(...)`.

- TanStack preloads the destination route's lazy chunk (fixes B), and
- runs the route's `loader` — which the shell already emits ONLY for pages
  with a `prefetch.rs` — so the `/__nx/prefetch` request becomes correctly
  conditional with no client-side route list (fixes A).

`nxPrefetch` keeps its in-flight dedup map and first-load skip; it still runs,
just via the loader. Loader wiring is unchanged.

## Implementation Notes

- **`preloadRoute` invocation form matters.** On the installed
  `@tanstack/react-router` (1.170.17), `preloadRoute(opts)` feeds `opts`
  straight into `buildLocation(opts)`, and `buildLocation` only reads
  `dest.to` / `dest.search` / etc. — it does NOT understand `href` (only
  `router.navigate` special-cases `href`). Passing `{ href }` silently builds
  the CURRENT location and preloads the wrong route. The handler must pass
  `{ to: url.pathname, search: Object.fromEntries(url.searchParams) }`; a
  concrete pathname as `to` is fine — `getMatchedRoutes` matches it against
  the route patterns.
- `preloadRoute` returns a promise that can reject (e.g. not-found) — swallow
  errors in the handler.
- **Chunk preload needs `component.preload`.** TanStack's `loadRouteChunk`
  warms `route.options.component?.preload?.()`. `nxLeaf` wraps
  `lazyRouteComponent` inside a plain function component, which has no
  `.preload` — forward it (`NxPage.preload = Lazy.preload`) or hover preloads
  the data but still not the chunk.
- Repeated hovers dedup twice over: TanStack's preload staleTime skips loader
  re-runs, and `nxPrefetch`'s in-flight map shares the request with the
  click-time loader call.
- The very first loader run (the document we hard-loaded) is still skipped by
  `nxFirstLoad` — that runs at initial mount through the loader path exactly
  as before.
- Fix the shell's misleading closing comment and the hover-handler comment
  block.
- `examples/react-todos` was all-seeded (every page had a `prefetch.rs`), so
  the mixed-app shape had no living reference. Add an unseeded page
  (`app/about/page.tsx`, no `prefetch.rs`) linked from the main page.

## Validation

- Unit (`crates/nextrs/src/bundle.rs`): the generated shell's mouseover
  handler calls `router.preloadRoute({ to: url.pathname, ... })` and no longer
  calls `nxPrefetch` directly; a loader is emitted only for prefetch-backed
  pages.
- e2e (`e2e/hover.mjs`, wired into CI next to the smoke):
  1. Hover a link to a route WITHOUT `prefetch.rs` → no request to
     `/__nx/prefetch` fires, and the destination chunk request DOES fire.
  2. Hover a link to a route WITH `prefetch.rs` → exactly one
     `/__nx/prefetch?path=...` request fires; hover twice, still one.
  3. Page load itself issues no `/__nx/prefetch` (first-load skip intact).
- Full smoke still green over both apps (site + react-todos, incl. the new
  /about route).
