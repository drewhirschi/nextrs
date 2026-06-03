# Manifest: nextrs

## Purpose

A **Next.js-like file-based routing framework for Rust**: folder-based routes, `page` / `layout` / `loading` / `middleware` conventions, and HTTP-level streaming that lets a route render its loading shell immediately and stream the real page when it's ready — without htmx, React, or any client-side framework. Deploys to Vercel as a single Rust serverless function with streaming preserved through Fluid compute.

The framework is built on Axum and Askama. Each segment under an `app/` directory is a route; convention files (`page.{rs,html}`, `layout.{rs,html}`, `loading.{rs,html}`, `middleware.rs`, `route.rs`) compose the response. `.rs` files hold logic; `.html` files are static fallbacks. Middleware runs before page rendering, loading-shell streaming, and `route.rs` handlers. When a loading slot exists, the framework streams the layout's "before children" half, the loading shell in a slot div, then (after awaiting the page) a `<template>` plus a ~200-byte inline `<script>` that swaps the slot, then the layout's "after children" half.

## Layout

Cargo workspace at the repo root. The root is also a package (the Vercel deployment target).

| Member | Purpose |
|---|---|
| `nextrs/` | The framework crate (library). Source at `nextrs/src/{lib,conventions,discovery,router,vercel,build}.rs`. The `vercel` and `build` modules are feature-gated. |
| `site/` | The consumer crate — currently the framework's own demo / docs site. Run with `cargo run --bin nextrs-dev` → http://localhost:3000. |
| (root package) | `nextrs-deploy` — single binary at `api/index.rs` that wraps the framework's axum router for `vercel_runtime::run`. |
| `dev/main.rs` | Repo-local dev watcher. Starts `cargo run -p site`, watches framework and demo app source paths, and restarts the child process on changes. |

The framework is a normal Rust library. The user writes only convention files (`app/.../{page,layout,loading}.{rs,html}`); `nextrs::build` (gated by the `build` feature, depended on from `[build-dependencies]`) runs at compile time via a tiny `build.rs` and emits the registry into `$OUT_DIR/nextrs_routes.rs`. The user's `main.rs` (or `api/index.rs`) does `include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"))` and calls `generated_registry()`. No `#[path]` mod declarations or `RouteEntry` constructors by hand.

## Conventions

Each folder under `app/` is a route segment. The framework looks for these files at each segment:

| File | Purpose | Variants |
|---|---|---|
| `page.{rs,html}` | The main content for the route | `.rs` (async handler) preferred; `.html` (static) fallback |
| `layout.{rs,html}` | Wraps this segment's page and any nested segments | same precedence |
| `loading.{rs,html}` | Shown while the page resolves (triggers streaming) | same precedence |
| `middleware.rs` | Request guard that may continue or return a response before rendering/streaming | `.rs` only |
| `route.rs` | API method handlers (POST/PUT/etc.) — `.rs` only |

Layout templates rendered through Askama must use `{{ children|safe }}` (not `{{ children }}`) so the framework's internal content marker passes through unescaped. Static layouts loaded via `nextrs::conventions::static_layout` accept either `{{children}}` or `{{ children }}` and do literal substitution (no askama escaping).

Dynamic URL segments use `[param]` directory naming (e.g. `app/users/[id]/page.rs` → `/users/{id}` in Axum's path syntax).

Middleware files export `pub async fn handle(req) -> nextrs::conventions::MiddlewareResult`. Root and nested middleware compose root-to-leaf; returning `MiddlewareResult::Response` short-circuits without rendering layouts, loading, pages, or API handlers, while `MiddlewareResult::Continue(req)` passes the request onward.

## Static assets

`site/public/` (colocated with `site/app/`) holds files served at root URL paths. The workspace-root `build.rs` mirrors them into `public/` at the workspace root via `nextrs::build::sync_public_dir`, so Vercel's CDN picks them up (verified `x-vercel-cache: HIT`, ~145ms warm TTFB). The workspace-root mirror is gitignored — generated artifact. Locally, `nextrs::router::build_router_with_public(registry, dir)` wires `tower-http::services::ServeDir` as a router fallback so the same paths resolve the same way.

## Vercel deployment

`nextrs::vercel::StreamingVercelLayer` (feature-gated, opt in with `nextrs = { features = ["vercel"] }`) is a drop-in replacement for `vercel_runtime::axum::VercelLayer`. The upstream layer only treats responses as streaming when content-type is `text/event-stream` or `application/json`; ours unconditionally streams the response body so HTML works. See `nextrs/src/vercel.rs` and `docs/streaming.md` for the full story.

`api/index.rs` is the deployed binary; `vercel.json` has a single catch-all rewrite to `/api/index`. Static files in `public/` take precedence over the rewrite (Vercel matches static files first).

## Where to look

| Area | File |
|---|---|
| Slot/file discovery | `nextrs/src/discovery.rs` — scans `app/` and produces `DiscoveredRoute { page, layout, loading, middleware, route }` where page/layout/loading track both `.rs` and `.html` paths |
| Route handler types | `nextrs/src/conventions.rs` — `PageFn`, `LayoutFn`, `LoadingFn`, `MiddlewareFn`, `RouteFn`; static helpers `static_page`, `static_layout`, `static_loading` |
| Routing + streaming | `nextrs/src/router.rs` — `build_router(registry) -> axum::Router`. Runs middleware, composes layouts around a content marker, splits on the marker, streams loading-then-page when a loading slot is present |
| Vercel adapter | `nextrs/src/vercel.rs` — `StreamingVercelLayer` (feature-gated by `vercel`). Drop-in replacement for `vercel_runtime::axum::VercelLayer` that doesn't buffer text/html |
| Progressive demo | `site/app/{simple, with-loading, with-layout}/` — three routes that progressively add `loading.{rs,html}` and `layout.{rs,html}`. The home page (`site/app/page.html`) is an overview with links and a per-route file listing |
| Codegen | `nextrs/src/build.rs` (feature `build`) — `emit_registry(app_dir, _, out_name)` walks `discover_routes` output and emits Rust source: `#[path]` mods for `.rs` slots, `static_*(include_str!(...))` for `.html` slots, and a `generated_registry()` function. Both paths emitted as absolute (necessary because `#[path]` inside an `include!`-d file resolves relative to the included file's location, not the includer). |
| Local site wiring | `site/src/main.rs` (33 lines) and `site/build.rs` — `include!` the generated file, call `generated_registry()` |
| Vercel deploy wiring | `api/index.rs` (22 lines) and root `build.rs` — same generated file, wrapped with `StreamingVercelLayer` for `vercel_runtime::run` |
| Askama configs | `site/askama.toml` (dirs = ["app"]); `askama.toml` at root (dirs = ["site/app"]) for the deploy binary |
| Streaming reference doc | `docs/streaming.md` — the model, layout-shell split, local vs Vercel, verification |
| Vercel deploy plan & results | `docs/vercel-deploy.md` — research findings, latency measurements, the VercelLayer bug story |

## Tests

`cargo test --workspace --all-features` (51 tests):

- Discovery: `.rs` + `.html` pairing, html-only segments, mixed nested, dynamic segments, API routes, middleware-only routes, empty-dir handling
- Conventions: static helpers (`static_page`, `static_layout` with both `{{children}}` and `{{ children }}` forms, `static_loading`), middleware result helpers
- Router: synchronous render, layout composition (1 / 3 levels deep), mixed static/dynamic layouts, layout-shell split-on-marker, streaming chunk ordering, multi-frame body, **timing-based proof that the loading shell arrives before the page handler resolves**, middleware redirects before loading, middleware continue preserving streaming, nested middleware ordering, request mutation through middleware, nested layouts under streaming, API methods, page+route on same path, `build_router_with_public` serves public-dir files on route miss and no-ops when the dir is absent
- Vercel (1): type-level smoke test that `StreamingVercelLayer` composes with axum routers
- Codegen: generated skeleton mentions every route + slot helper, middleware wiring, `.rs` wins over `.html` when both are present, route.rs methods, all emitted paths are absolute

## Non-goals

- **No client-side framework.** No htmx, no React, no JS bundle. The loading→page swap is a single inline script the framework emits; nothing else runs in the browser.
- **Not pinning a public API.** Types and helpers may move as conventions harden.

## Roadmap (rough)

1. **`error.{rs,html}`** segment convention.
2. **Per-route Vercel binaries** as an option for very large apps (current single-binary fits everything we need now).
3. **Dev-server ergonomics**: the repo-local `nextrs-dev` watcher restarts `cargo run -p site` when Rust, template, or public asset files change. Further polish could move this into a proper published CLI.
4. **Suspense-style nested streaming**: today there's exactly one loading slot per route. React Server Components-style nested boundaries would need a more sophisticated streaming protocol.
5. **`route.rs` codegen hardening.** Basic API method codegen is in place; future work is richer diagnostics and request extraction conventions.
6. **Upstream**: file an issue/PR with the Vercel team to make `vercel_runtime::axum::VercelLayer` recognize `text/html` for streaming (or take an `always_stream` flag), so `nextrs::vercel::StreamingVercelLayer` becomes optional.
