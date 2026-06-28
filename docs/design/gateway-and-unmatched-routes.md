# Design: gateways & unmatched routes

Status: **decision doc** — the `not-found` convention (Option A) is implemented
on `feat/not-found-convention`, re-built against current `main`. Other options
remain proposals. Originally written 2026-06-15; re-implemented and revised for
the post-refactor router/discovery/build on `main`.

## The question

> "Isn't all we need is a way to catch unmatched routes? What are the
> conventions of Next.js for that?"

Two different needs hide under "catch unmatched routes," and Next.js solves
them with *different* primitives. nextrs already covers one and a half of them.
This doc lays out the conventions, maps each to nextrs's current state, and
proposes a build order.

---

## What Next.js actually provides

Next has **five** distinct mechanisms. They are not interchangeable — each owns
a different slice of "the request didn't land on a normal page."

| # | Convention | Runs… | Owns | Typical use |
|---|---|---|---|---|
| 1 | `[...slug]/page.tsx` (catch-all) | on a route that matched a wildcard | a *known subtree* | docs/CMS pages, `/blog/2024/06/post` |
| 2 | `[[...slug]]/page.tsx` (optional catch-all) | matches the parent **and** all descendants, incl. empty | a subtree *plus its root* | i18n root, fully dynamic site |
| 3 | catch-all **Route Handler** `[...all]/route.ts` | on matched wildcard, any method | a *delegated API subtree* | `app/api/auth/[...all]/route.ts` — hand the whole prefix to one library |
| 4 | `not-found.tsx` + `notFound()` | when nothing matched, or code calls `notFound()` | the *404 surface* | branded 404, "this booking doesn't exist" |
| 5 | `middleware.ts` + `config.matcher` | **before** routing, on every matching URL | *rewrite / redirect / proxy* | auth gate, A/B, locale rewrite, reverse proxy |

The key distinction is **#1–3 are matched routes** (the router picked them; the
URL is "known") versus **#4–5 fire on the un-routed request** (#4 after the
router gives up, #5 before the router runs at all).

"Catch unmatched routes" almost always means **#4 (a real 404 surface)** or
**#5 (intercept arbitrary paths before routing)** — not the catch-all segments,
which only fire inside a subtree you already declared.

---

## Where nextrs stands today

- **#1 catch-all `[...x]`** — ✅ **built**. `dir_name_to_segment` and the macro's
  `url_from_file` both map `[...all]` → `{*all}` (Axum wildcard). Tests:
  `discovery.rs::test_dynamic_and_catch_all_segments`,
  `nextrs-macros::url_maps_catch_all_segments`.
- **#3 catch-all Route Handler** — ✅ falls out of #1 for free.
  `app/api/auth/[...all]/route.rs` with method handlers already works once the
  wildcard segment routes. This is the "own a subtree like `/api/auth/*`"
  gateway.
- **#2 optional catch-all `[[...x]]`** — ❌ **not built**. `dir_name_to_segment`
  strips a single `[...]`; doubled brackets aren't recognized and there's no
  "matches the parent too" expansion. Axum has no native optional-wildcard, so
  this needs **two registered routes** per optional-catch-all dir (`/x` and
  `/x/{*rest}`) pointing at the same handler.
- **#4 not-found surface** — ✅ **built** (`feat/not-found-convention`).
  `not-found.{rs,html,tsx}` is a discovery slot; the router installs a fallback
  that picks the deepest-ancestor not-found of the unmatched path, wraps it in
  that segment's layouts, and responds `404`. Works under `build_router`,
  `build_router_with_prefetch`, and `build_router_with_public` (ServeDir miss →
  not-found). Tests across discovery/conventions/router/build/bundle. **Remaining:**
  no server-side `notFound()` escape hatch (trigger the surface from inside a
  page).
- **#5 pre-route interceptor** — ❌ **doesn't exist as a primitive**. nextrs
  middleware is **per-directory and only runs on matched routes**
  (`run_middlewares` is called from inside `render_route` / `handle_method_route`).
  A request that matches *no* route never sees any middleware. There is no
  equivalent to top-level `middleware.ts` that can rewrite/redirect/proxy an
  arbitrary URL before the router decides.

So: the "own a subtree" gateway is basically done. The "404 surface" is now
built; the "intercept anything before routing" gateway is still genuinely
missing.

---

## Option A — `not-found.{rs,html,tsx}` convention (#4) — ✅ DONE

Implemented as described below. Models not-found as a `RouteRegistry`-level
collection (`add_not_found(path, render)`) rather than a `RouteEntry` field, so
the existing `RouteEntry` literals (and the ~30 router unit tests that construct
them) stayed untouched. Remaining work is just the `notFound()` escape hatch.

**What:** a new discovery slot. `app/not-found.rs` (and nested
`app/admin/not-found.rs`) registers a fallback that renders when no route
matches, wrapped in the layouts of its directory — exactly like a page. All
three rendering models are supported, mirroring `page`:

- `not-found.rs` — a Rust handler exporting `pub async fn render(req) -> String`.
- `not-found.html` — a static file, baked via `include_str!` + `static_page`.
- `not-found.tsx` — a client-rendered React page; gets the same shell a
  `page.tsx` gets (mount div, error boundary, stylesheet) and is bundled under a
  distinct slug so a segment can carry both a `page.tsx` and a `not-found.tsx`.

**Why it's the cheapest real win:** every app needs a branded 404. It's additive
(new optional file, no behavior change for apps without it) and it's the
convention users most expect to "just work."

### Shape (as built on `main`)

- **`conventions.rs`** — a `NotFoundEntry { path, render: PageFn }` type and a
  `RouteRegistry.not_found: Vec<NotFoundEntry>` collection, with
  `RouteRegistry::add_not_found(path, render)`. The render is a `PageFn` — the
  same shape as a page — so all three variants reuse the existing page helpers
  (`static_page`, the `.rs` `render(req)` call, the tsx shell).
- **`discovery.rs`** — a `not_found: Slot` field on `DiscoveredRoute`, scanning
  `not-found.{rs,html,tsx}` per directory. A directory with only a `not-found`
  file still registers a route (added to the slot-existence check).
- **`router.rs`** — the route-building loop was extracted into
  `build_route_table(entries, prefetch)` so the not-found surfaces can be wired
  as the fallback separately. `with_not_found_fallback(router, entries, not_found,
  prefetch)` installs an Axum `.fallback(...)` only when surfaces exist
  (otherwise the bare Axum `404` is preserved). `render_not_found` picks the
  entry whose declaring path is the **deepest** ancestor of the requested path
  (longest-prefix match via the existing `entry_applies_to_path` + `route_depth`),
  wraps the rendered body in that segment's layouts via
  `collect_layouts_for_path` / `layout_shell`, injects speculation-rules into the
  `<head>` (same as a page response — a no-op for head-less fragments / prefetch
  off), and responds `404`. Per-directory middleware does **not** run for an
  unmatched path — middleware is scoped to matched routes, and nothing matched.
- **`build_router_with_public`** — when a `public/` dir exists, ServeDir is
  given a `not_found_service` so a static-file miss falls through to the
  not-found surfaces rather than ServeDir's bare `404`. With no surfaces
  registered, behavior is unchanged.
- **`build.rs`** — `emit_not_found(out, idx, route)` emits a
  `registry.add_not_found(path, render)` call per segment: `.rs` via a `#[path]`
  module's `render(req)`, `.html` via `include_str!` + `static_page`, `.tsx` via
  a `tsx_page_shell(not_found_slug(path))` shell. A `not_found_slug` helper
  (`/` → `not-found`, `/admin` → `admin-not-found`) keeps the not-found bundle
  from colliding with the page bundle. A codegen conflict is raised when
  `not-found.tsx` coexists with `not-found.rs`/`not-found.html` (one rendering
  model per segment, matching the page/layout/loading conflict checks).
- **`bundle.rs`** — `page_bundles` now emits a browser bundle for each
  `not-found.tsx` as well as each `page.tsx`, under `not_found_slug`, composing
  the segment's `.tsx` layouts client-side exactly like a page.

### Design decisions / deviations from the original sketch

- **Prefetch injection on the 404 surface.** `main`'s router injects
  speculation-rules into every full-document page response; the re-implementation
  threads the same `PrefetchConfig` into `render_not_found` so a 404 wrapped in a
  layout with a `<head>` behaves like any other full document. (The original
  predated the prefetch feature and didn't do this.)
- **tsx shell reuses `tsx_page_shell`.** The original emitted a minimal mount-div
  shell. To stay consistent with how `page.tsx` renders on `main`, the not-found
  tsx variant uses the same shell — stylesheet link, error boundary, module
  script — pointing at the `not_found_slug` bundle.
- **No `props.rs` for not-found.** A not-found surface is propless (no server
  seed await), matching Next's `not-found.tsx` (a Server Component with no params
  by default). The contextual case is the `notFound()` escape hatch below.

**Open question:** does `not-found.rs` get the request (for "booking 31 doesn't
exist" messaging) or is it static? The handler signature *is* `PageFn`, so an
`.rs` handler already receives the request; the missing piece is a server-side
`notFound()` that a matched page/route can call to bail into the nearest surface
with context. Recommend: ship the convention now (done), add `notFound()` next.

---

## Option B — pre-route interceptor (#5), the real `middleware.ts` analog

**What:** a single top-level hook that runs **before** the Axum router, on every
request, matched or not. It can: continue, redirect, rewrite (change the path the
router then matches), or proxy to an upstream. This is the "gateway" in the
reverse-proxy sense.

**Why it's the meatier one:** it's the only thing that unlocks
rewrite/proxy/global-auth-gate, and it's the piece most people *mean* by
"gateway." But it overlaps conceptually with per-directory middleware, so the
design has to be deliberate or we end up with two confusing middleware systems.

**Shape (Axum `Router::layer` / `from_fn`, wrapping the whole router):**
- New convention file: `app/gateway.rs` (deliberately **not** `middleware.rs`, to
  keep "runs before routing, on everything" distinct from "per-subtree, on
  matched routes"). Returns the same `MiddlewareResult` enum, plus a third
  variant `Rewrite(uri)` that mutates the request URI before it hits the router.
- `router.rs`: `build_router` wraps the final `Router` in
  `axum::middleware::from_fn` that calls the gateway fn, handling
  Continue/Response/Rewrite. Rewrite re-dispatches by editing `req.uri`.
- Proxy is a special case of Response: the gateway fn does its own
  `reqwest`/`hyper` upstream call and returns the response. (We can ship
  Continue/Response/Rewrite first and leave proxy as "do it yourself in the fn.")

**Cost:** medium. The wiring is small; the *design* care is the cost — naming,
the rewrite-loop guard, ordering vs. per-dir middleware, and docs that make the
two middleware tiers legible.

**Interaction with the existing per-dir middleware:** gateway runs first (before
routing) → router matches → per-dir middleware runs (after match, before
handler). Clean two-tier story: **gateway = edge/global, middleware = subtree.**

Note the ordering interaction with **#4**: a gateway runs before routing, so it
sees the request *before* the not-found fallback would. An unmatched URL that the
gateway leaves alone still lands on the not-found surface as today.

---

## Adjacent: dev-mode "which middleware touched this" header

Independently requested. Both tiers should be observable in dev. Under
`#[cfg(debug_assertions)]`, stamp:

- `x-nextrs-middleware: /, /admin, /admin/users` — the per-dir chain that ran
  (needs `collect_middlewares_for_path` to also return the matched entry paths).
- `x-nextrs-gateway: rewrite /a→/b` (if Option B lands) — what the gateway did.

Tiny: a `stamp_trace(resp, paths)` helper applied in both response paths of
`render_route` / `handle_method_route`. Gated to debug builds so prod is
untouched. **This one is cheap and independently useful — worth doing regardless
of A/B.**

---

## Recommended build order

1. **Dev-mode middleware trace header** — cheap, independently useful, no design
   risk. Lands the per-dir `x-nextrs-middleware` half now; gateway half later.
2. **`not-found` convention (Option A)** — the "catch unmatched routes" most
   people actually want, cheap and additive. ✅ **done** on
   `feat/not-found-convention`.
3. **Pre-route interceptor (Option B)** — the real gateway; do it last and
   deliberately, with the two-tier middleware story documented.

If "gateway" to you means **"own `/api/auth/*` with one handler,"** that's built
(#1/#3). If it means **"rewrite/proxy/globally gate arbitrary paths before
routing,"** that's Option B and the one new primitive.

---

## Decisions needed

- [ ] Which gateway is the actual target — subtree (#1/#3, done) or pre-route
  interceptor (#5, new)?
- [x] Ship the `not-found` convention now — **done** (`.rs`/`.html`/`.tsx`,
  subtree-scoped, `404`).
- [ ] Server-side `notFound()` escape hatch for the contextual 404 case.
- [ ] `gateway.rs` as a separate convention vs. overloading `middleware.rs` at
  the root — recommend separate, to keep before-routing vs. on-match legible.
- [ ] Is the dev trace header greenlit to build standalone? (recommend yes)
