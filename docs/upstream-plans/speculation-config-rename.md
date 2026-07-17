# Three mechanisms named "prefetch" — rename PrefetchConfig to SpeculationConfig

- **Reported-in:** hand-assembled nextrs port (agent session)
- **Date:** 2026-07-16
- **Status:** open

## Problem

nextrs grew three ahead-of-time mechanisms and called all of them "prefetch":

1. **Data seeding** — `prefetch.rs` beside a `page.tsx`, the `/__nx/prefetch`
   endpoint, `QuerySeed` streaming. Warms the React Query cache.
2. **Route chunk warming** — `router.preloadRoute` on hover (PR #33). Warms the
   JS chunk for a React route.
3. **Document-level browser Speculation Rules** — the injected
   `<script type="speculationrules">`, configured by a type named
   `PrefetchConfig`. Warms the *next full document*.

A real port was burned by the collision: the porting agent set
`PrefetchConfig::OFF` expecting it to silence the `/__nx/prefetch` endpoint
(mechanism 1). It doesn't — it disables mechanism 3, which the port didn't even
know existed. Nothing in the names distinguishes them.

Two adjacent problems surfaced by the same audit:

- **Stale module docs.** `prefetch.rs`'s header still says "nextrs has no
  client router: every `<a>` click is a full-document HTTP GET" — false since
  the app-shell/soft-nav feature (PR #29/#33). React-routed pages are
  soft-navigated; a speculatively fetched document for them is never used.
- **On-by-default waste.** `PrefetchConfig::default()` injects
  Prefetch/Moderate rules into every full-document response, matching *all*
  same-origin links — including links to React app-shell routes, where the
  click interceptor means the prefetched document is discarded. That's a full
  server render per hover, on metered compute, double-firing alongside
  `preloadRoute`.

## Proposed Direction

Ecosystem-aligned naming (Next.js: "prefetch" = data; TanStack Router:
"preload" = route code; the browser spec: "Speculation Rules"):

| term | mechanism | disposition |
|---|---|---|
| **prefetch** | data seeding (`prefetch.rs`, `/__nx/prefetch`) | stays as-is |
| **preload** | route chunk warming (`router.preloadRoute`) | stays (PR #33); never call it prefetch |
| **speculation** | document-level browser Speculation Rules | renamed from `PrefetchConfig` |
| **seed** | the payload noun (`QuerySeed`, `seed_key`) | stays as-is (decided) |

Changes:

1. `PrefetchConfig` → `SpeculationConfig`; module `prefetch.rs` →
   `speculation.rs`; `build_router_with_prefetch` →
   `build_router_with_speculation`. Deprecated aliases for one release
   (`pub type PrefetchConfig = SpeculationConfig;`, a deprecated
   `build_router_with_prefetch` wrapper, a deprecated `nextrs::prefetch`
   module path).
2. **Default flips to Off** (behavior change, rides 0.3.8). Speculation is
   demoted to an opt-in for server-rendered apps; React-routed pages already
   get chunk preload + data prefetch via the app shell.
3. Module docs rewritten to position speculation as that opt-in.
4. When enabled, the injected rules **exclude React app-shell routes** via
   `"where": {"and": [..., {"not": {"href_matches": [patterns]}}]}` — the
   registry now carries the React page paths (same discovery that builds
   `NX_APP_ROUTES`), converted to URL Pattern syntax (`{id}` → `:id`,
   `{*rest}` → `*`).
5. Internal `props` field (discovery/build/bundle) renamed to `prefetch` —
   it was two renames behind the user-facing term. The user-facing legacy
   `props.rs` filename convention still works.

## Implementation Notes

- `SpeculationConfig::resolve(&registry.react_pages)` computes the final
  script once at router build; render paths inject a precomputed
  `ResolvedSpeculation` instead of re-serializing per request.
- `build_router_with_public` gains a `build_router_with_public_and_speculation`
  sibling so apps using the public-dir path (the docs site) can opt in.
- The docs site opts in EXPLICITLY (`SpeculationConfig { mode: Prefetch,
  eagerness: Moderate }`) — its /docs pages are server-rendered and benefit;
  its `/` React landing is excluded by the new scoping. react-todos is
  all-React and needs nothing: default-off just removes rules its interceptor
  was discarding anyway.

### Behavior change (0.3.8 changelog note)

> **Speculation Rules are now off by default.** Previously every
> full-document response got a `<script type="speculationrules">` injected
> (prefetch on hover). Apps that want it back opt in:
> `build_router_with_speculation(registry, SpeculationConfig { mode:
> SpeculationMode::Prefetch, eagerness: Eagerness::Moderate })`. When enabled,
> rules now exclude React app-shell routes (the soft-nav interceptor made
> those document prefetches pure waste).

## Validation

- `cargo test --workspace --features nextrs/build,nextrs/tsx` (CI feature set)
  — includes new unit tests: default is Off; enabled app with mixed
  React/server routes emits rules excluding React paths (incl. dynamic
  segments) while still matching server paths; deprecated old names compile
  (warnings, not errors).
- `cargo build -p site` (bundling ON) + boot: landing page and a /docs page
  render; docs pages' HTML contains the speculationrules script (site opted
  in) with `/` (the React landing) excluded from `href_matches`.
- `cargo build -p react-todos` + boot: NO speculationrules script anywhere
  (default off).
- `node e2e/smoke.mjs` and `node e2e/hover.mjs` green.
