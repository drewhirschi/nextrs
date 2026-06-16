# Design: gateways & unmatched routes

Status: **draft / decision doc** — nothing here is built unless it links to a
landed commit. Written 2026-06-15.

## The question

> "Isn't all we need is a way to catch unmatched routes? What are the
> conventions of Next.js for that?"

Two different needs hide under "catch unmatched routes," and Next.js solves
them with *different* primitives. nextrs already covers one and a half of them.
This doc lays out the conventions, maps each to nextrs's current state, and
proposes a build order.

---

## What Next.js actually provides

Next has **four** distinct mechanisms. They are not interchangeable — each owns
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

- **#1 catch-all `[...x]`** — ✅ **built**, ⏳ **unpublished**. `dir_name_to_segment`
  and the macro's `url_from_file` both map `[...all]` → `{*all}` (Axum wildcard).
  Tests: `discovery.rs::test_dynamic_and_catch_all_segments`,
  `nextrs-macros::url_maps_catch_all_segments`. Shipped in working tree, **not in
  crates.io 0.2.0** — consumers (incl. the bonaparte migration) need **0.2.1**.
- **#3 catch-all Route Handler** — ✅ falls out of #1 for free. `app/api/auth/[...all]/route.rs`
  with method handlers already works once the wildcard segment routes. This is
  the "own a subtree like `/api/auth/*`" gateway — the hhh native-auth port wants
  exactly this slot (`app/api/auth/` is currently an open directory).
- **#2 optional catch-all `[[...x]]`** — ❌ **not built**. `dir_name_to_segment`
  strips a single `[...]`; doubled brackets aren't recognized and there's no
  "matches the parent too" expansion. Axum has no native optional-wildcard, so
  this needs **two registered routes** per optional-catch-all dir (`/x` and
  `/x/{*rest}`) pointing at the same handler.
- **#4 not-found surface** — ✅ **built 2026-06-15**, ⏳ **unpublished**.
  `not-found.{rs,html,tsx}` is now a discovery slot; the router installs a
  fallback that picks the deepest-ancestor not-found of the unmatched path,
  wraps it in that segment's layouts, and responds `404`. Works under
  `build_router` and `build_router_with_public` (ServeDir miss → not-found).
  Tests across discovery/conventions/router/build/bundle. Ships in `0.2.1`. See
  §5.3 of the migration guide. **Remaining:** no server-side `notFound()`
  escape hatch (trigger the surface from inside a page).
- **#5 pre-route interceptor** — ❌ **doesn't exist as a primitive**. nextrs
  middleware is **per-directory and only runs on matched routes**
  (`run_middlewares` is called from inside `render_route` / `handle_method_route`).
  A request that matches *no* route never sees any middleware. There is no
  equivalent to top-level `middleware.ts` that can rewrite/redirect/proxy an
  arbitrary URL before the router decides.

So: the "own a subtree" gateway is basically done (publish-gated). The "404
surface" and the "intercept anything before routing" gateways are genuinely
missing.

---

## Option A — `not-found.rs` convention (#4) — ✅ DONE (2026-06-15)

Implemented as described below. Models not-found as a `RouteRegistry`-level
collection (`add_not_found(path, render)`) rather than a `RouteEntry` field, so
the ~40 existing `RouteEntry` literals stayed untouched. Remaining work is just
the `notFound()` escape hatch and publishing `0.2.1`.

**What:** a new discovery slot. `app/not-found.rs` (and nested
`app/admin/not-found.rs`) registers a fallback that renders when no route
matches, wrapped in the layouts of its directory — exactly like a page. Plus a
`not_found()` escape hatch a page/route can call to bail into the nearest
not-found surface.

**Why it's the cheapest real win:** every app needs a branded 404; hhh already
proved the need by hand-rolling one. It's additive (new optional file, no
behavior change for apps without it) and it's the convention users most expect
to "just work."

**Shape:**
- `discovery.rs`: add `not_found: optional_path(current, "not-found.rs")` to the
  per-dir scan; thread into `RouteEntry` (or a sibling registry).
- `router.rs`: install the **deepest** not-found handler as the Axum
  `.fallback(...)`. For nested not-founds, pick by longest-prefix match on the
  un-routed path (mirror `entry_applies_to_path`), then wrap in
  `collect_layouts_for_path`. Set status `404`.
- `not_found()`: a sentinel `Response` (or panic-with-catch) the page layer maps
  to the same fallback. MVP can ship the convention without the escape hatch.

**Cost:** small. One discovery field, one fallback install, layout reuse already
exists. ~½ day.

**Open question:** does `not-found.rs` get the request (for "booking 31 doesn't
exist" messaging) or is it static? Next's is a Server Component with no params
by default; `notFound()` is how you trigger it with context. Recommend: static
shell for the convention, `not_found()` for the contextual case.

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
two middleware tiers legible. ~1–2 days incl. the dev-mode trace header below.

**Interaction with the existing per-dir middleware:** gateway runs first (before
routing) → router matches → per-dir middleware runs (after match, before
handler). Clean two-tier story: **gateway = edge/global, middleware = subtree.**

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

1. **Publish 0.2.1** — unblocks the catch-all subtree gateway (#1/#3) for all
   consumers + the bonaparte migration. Pure release work, no new code.
2. **Dev-mode middleware trace header** — cheap, independently useful, no design
   risk. Lands the per-dir `x-nextrs-middleware` half now; gateway half later.
3. **`not-found.rs` convention (Option A)** — the "catch unmatched routes" most
   people actually want, cheap and additive.
4. **Pre-route interceptor (Option B)** — the real gateway; do it last and
   deliberately, with the two-tier middleware story documented.

If "gateway" to you means **"own `/api/auth/*` with one handler,"** you're at
step 1 — it's built, just publish. If it means **"rewrite/proxy/globally gate
arbitrary paths before routing,"** that's step 4 and the one new primitive.

---

## Decisions needed

- [ ] Which gateway is the actual target — subtree (#1/#3, ~done) or pre-route
  interceptor (#5, new)?
- [x] Ship `not-found.rs` now — **done 2026-06-15** (`.rs`/`.html`/`.tsx`).
- [ ] `gateway.rs` as a separate convention vs. overloading `middleware.rs` at
  the root — recommend separate, to keep before-routing vs. on-match legible.
- [ ] Is the dev trace header greenlit to build standalone? (recommend yes)
