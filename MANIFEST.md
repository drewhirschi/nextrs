# Manifest: nextrs

## Purpose

**nextrs** is a Rust web framework for building React apps. You get the Next.js developer experience — file-based routing, `page` / `layout` / `loading` conventions, one-command Vercel deploys with zero infra — but the server is Rust. The client borrows the best of the TanStack ecosystem: **Query** for data (server-prefetched into the cache) and **Router** for instant navigation.

**Engineered for agents.** Software gets built differently now: AI agents add features faster than a Next.js/Node codebase can absorb — build times balloon, the runtime slows, things get fragile, and a lot of effort goes into just keeping it from falling apart. Rust is orders of magnitude faster by design, so that headroom means agent-generated code stays fast and doesn't rot. Built for app-style products — dashboards, internal tools, anything behind auth.

> The "Layout / Conventions / Streaming" sections below predate the React-first rewrite (they describe the original HTML/Askama streaming design) and need a code-verified update.

## Layout

Cargo workspace at the repo root. The root is also a package (the Vercel deployment target).

| Member | Purpose |
|---|---|
| `nextrs/` | The framework crate (library). Source at `nextrs/src/{lib,conventions,discovery,router,seed,prefetch,openapi,vercel,build,docs,bundle}.rs`. `vercel`, `build` and `docs` (both gated by the `build` feature), and `bundle` (gated by the `tsx` feature) are feature-gated. |
| `site/` | The consumer crate — currently the framework's own demo / docs site. Run with `cargo dev` → http://localhost:3000. |
| (root package) | `nextrs-deploy` — single binary at `api/index.rs` that wraps the framework's axum router for `vercel_runtime::run`. |
| `xtask/` | Repo-local dev watcher. Starts `cargo run -p site`, watches framework and demo app source paths, and restarts the child process on changes. |

The framework is a normal Rust library. The user writes only convention files (`app/.../{page,layout,loading}.{rs,html}`); `nextrs::build` (gated by the `build` feature, depended on from `[build-dependencies]`) runs at compile time via a tiny `build.rs` and emits the registry into `$OUT_DIR/nextrs_routes.rs`. The user's `main.rs` (or `api/index.rs`) does `include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"))` and calls `generated_registry()`. No `#[path]` mod declarations or `RouteEntry` constructors by hand.

## Conventions

Each folder under `app/` is a route segment. The framework looks for these files at each segment:

| File | Purpose | Variants |
|---|---|---|
| `page.{rs,html,tsx}` | The main content for the route | `.rs` (async handler) / `.html` (static Askama) / `.tsx` (React, bundled to `/dist/<slug>.js` and mounted into `<div id="__nx_root__">`; requires the `tsx` feature) |
| `layout.{rs,html,tsx}` | Wraps this segment's page and any nested segments | same `.rs`/`.html`/`.tsx` variants |
| `loading.{rs,html,tsx}` | Shown while the page resolves (triggers streaming) | same `.rs`/`.html`/`.tsx` variants |
| `props.rs` | Server data for a sibling `page.tsx` — exports `pub async fn props(req) -> nextrs::QuerySeed`; streamed as a JSON `<script id="__nx_seeds__">` and loaded into the React Query cache before mount | `.rs` only |
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
| Slot/file discovery | `nextrs/src/discovery.rs` — scans `app/` and produces `DiscoveredRoute { page, layout, loading, middleware, route, props }` where page/layout/loading are each a `Slot { rs, html, tsx }` (every variant optional) and `props` is the `props.rs` path |
| Route handler types | `nextrs/src/conventions.rs` — `PageFn`, `LayoutFn`, `LoadingFn`, `MiddlewareFn`, `RouteFn`; static helpers `static_page`, `static_layout`, `static_loading` |
| Routing + streaming | `nextrs/src/router.rs` — `build_router(registry) -> axum::Router` (and `build_router_with_prefetch` / `build_router_with_public`). Runs middleware, composes layouts around a content marker, splits on the marker, streams loading-then-page when a loading slot is present |
| React bundling | `nextrs/src/bundle.rs` (feature `tsx`) — `bundle_pages(BundleConfig)`. For each `page.tsx` / `loading.tsx` emits an entry wrapper (layout composition + `QueryClientProvider` + seed hydration + `createRoot` mount) and runs the embedded rolldown bundler to produce `/dist/<slug>.js` |
| Server data seeding | `nextrs/src/seed.rs` — `QuerySeed`, `SeedEntry`, `seed_key`; the value a `props.rs` returns, serialized into a `<script id="__nx_seeds__">` tag and loaded into the React Query cache before mount |
| Navigation prefetch | `nextrs/src/prefetch.rs` — `PrefetchConfig`, `SpeculationMode`, `Eagerness`; injects a `<script type="speculationrules">` for browser-native prefetch/prerender (no client router) |
| OpenAPI serving | `nextrs/src/openapi.rs` — `spec_router(generated_openapi())` serves the codegen-built OpenAPI document at `/openapi.json` (consumed by the typed client) |
| Docs pipeline | `nextrs/src/docs.rs` (feature `build`) — `emit_docs(DocsConfig)` renders markdown once into both the docs-UI slices and the `llms.txt` / `llms-full.txt` files |
| Vercel adapter | `nextrs/src/vercel.rs` — `StreamingVercelLayer` (feature-gated by `vercel`). Drop-in replacement for `vercel_runtime::axum::VercelLayer` that doesn't buffer text/html |
| Progressive demo | `site/app/{simple, with-loading, with-layout}/` — three routes that progressively add `loading.{rs,html}` and `layout.{rs,html}`. The home page (`site/app/page.html`) is an overview with links and a per-route file listing |
| Codegen | `nextrs/src/build.rs` (feature `build`) — `emit_registry(app_dir, _, out_name)` walks `discover_routes` output and emits Rust source: `#[path]` mods for `.rs` slots, `static_*(include_str!(...))` for `.html` slots, and a `generated_registry()` function. Both paths emitted as absolute (necessary because `#[path]` inside an `include!`-d file resolves relative to the included file's location, not the includer). |
| Local site wiring | `site/src/main.rs` (33 lines) and `site/build.rs` — `include!` the generated file, call `generated_registry()` |
| Vercel deploy wiring | `api/index.rs` (22 lines) and root `build.rs` — same generated file, wrapped with `StreamingVercelLayer` for `vercel_runtime::run` |
| Askama configs | `site/askama.toml` (dirs = ["app"]); `askama.toml` at root (dirs = ["site/app"]) for the deploy binary |
| Streaming reference doc | `docs/streaming.md` — the model, layout-shell split, local vs Vercel, verification |
| Vercel deploy plan & results | `docs/vercel-deploy.md` — research findings, latency measurements, the VercelLayer bug story |

## Tests

`cargo test --workspace --all-features` (~121 tests across `nextrs` + `nextrs-macros`):

- Discovery: `.rs` + `.html` pairing, html-only segments, mixed nested, dynamic segments, API routes, middleware-only routes, empty-dir handling
- Conventions: static helpers (`static_page`, `static_layout` with both `{{children}}` and `{{ children }}` forms, `static_loading`), middleware result helpers
- Router: synchronous render, layout composition (1 / 3 levels deep), mixed static/dynamic layouts, layout-shell split-on-marker, streaming chunk ordering, multi-frame body, **timing-based proof that the loading shell arrives before the page handler resolves**, middleware redirects before loading, middleware continue preserving streaming, nested middleware ordering, request mutation through middleware, nested layouts under streaming, API methods, page+route on same path, `build_router_with_public` serves public-dir files on route miss and no-ops when the dir is absent
- Vercel (1): type-level smoke test that `StreamingVercelLayer` composes with axum routers
- Codegen: generated skeleton mentions every route + slot helper, middleware wiring, `.rs` wins over `.html` when both are present, route.rs methods, all emitted paths are absolute

## Non-goals

- **No JS server runtime.** The deployed binary is Rust/axum — there is no Node server in production. React `.tsx` pages are bundled to static JS *at build time* by the embedded rolldown bundler (feature `tsx`) and served as plain files; the server never executes JS.
- **No required client JS for `.rs`/`.html` pages.** Askama pages render to HTML on the server; the loading→page swap is a single inline script the framework emits, and navigation prefetch is browser-native Speculation Rules — so non-React routes ship no client framework or bundle.
- **Not pinning a public API.** Types and helpers may move as conventions harden.

## Roadmap

See `ROADMAP.md` for the working roadmap. Current deferred items include React
HMR/Fast Refresh, a first-class app scaffolder command, `error.{rs,html}`,
per-route Vercel binaries, richer `route.rs` diagnostics, nested streaming, and
upstream Vercel adapter support for streaming `text/html`.
