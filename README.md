# nextrs

**nextrs** is a Rust web framework for building React apps. You get the Next.js developer experience — file-based routing, `page` / `layout` / `loading` conventions, one-command Vercel deploys with zero infra — but the server is Rust. The client borrows the best of the TanStack ecosystem: **Query** for data (server-prefetched into the cache) and **Router** for instant navigation.

**Engineered for agents.** Software gets built differently now: AI agents add features faster than a Next.js/Node codebase can absorb — build times balloon, the runtime slows, things get fragile, and a lot of effort goes into just keeping it from falling apart. Rust is orders of magnitude faster by design, so that headroom means agent-generated code stays fast and doesn't rot. Built for app-style products — dashboards, internal tools, anything behind auth.

> Note: the sections below still describe an earlier HTML/streaming-first version of nextrs and are being rewritten React-first.

## Quick look

```
site/app/
├── middleware.rs                ← global request guard, optional
├── page.{rs,html}              ← /
├── layout.{rs,html}            ← root layout, applied to every route
├── simple/
│   └── page.{rs,html}          ← /simple — just a page
├── with-loading/
│   ├── page.{rs,html}          ← /with-loading — page + streaming loading shell
│   └── loading.{rs,html}
└── with-layout/
    ├── layout.{rs,html}        ← /with-layout — adds a sidebar
    ├── page.{rs,html}
    └── loading.{rs,html}
```

Each folder is a route segment. Each file is a convention slot:

| File | Purpose | Variants |
|---|---|---|
| `page.{rs,html}` | The main content | `.rs` (async handler) preferred; `.html` (static) fallback |
| `layout.{rs,html}` | Wraps the segment's page and any nested segments | same |
| `loading.{rs,html}` | Triggers streaming — shown while the page resolves | same |
| `middleware.rs` | Runs before page rendering, loading streaming, and API handlers | `.rs` only |
| `route.rs` | API method handlers (POST/PUT/etc.) | `.rs` only |

`.rs` files are Rust handlers (typically askama templates with logic). `.html` files are static fallbacks. When both exist for a slot, `.rs` wins.

## API routes

Add a `route.rs` file under `app/` to handle non-page HTTP endpoints:

```text
site/app/api/ping/route.rs   ← /api/ping
```

```rust
use axum::body::Body;
use axum::response::IntoResponse;
use http::{Request, StatusCode};

pub async fn get(_req: Request<Body>) -> impl IntoResponse {
    StatusCode::OK
}

pub async fn post(_req: Request<Body>) -> impl IntoResponse {
    StatusCode::CREATED
}
```

Supported method function names are `get`, `post`, `put`, `patch`, `delete`, `head`, and `options`. `page.rs` owns `GET` for UI routes, so `page.rs` and `route.rs` may coexist only when `route.rs` does not export `get`.

## Middleware

Add `middleware.rs` to any route segment to guard that segment and its nested routes. Middleware composes root-to-leaf and runs before layouts, loading shells, pages, and `route.rs` handlers, so redirects and auth failures can return real HTTP responses before streaming commits headers.

```rust
use axum::response::{IntoResponse, Redirect};
use nextrs::conventions::MiddlewareResult;

pub async fn handle(
    req: http::Request<axum::body::Body>,
) -> MiddlewareResult {
    let has_session = req.headers().get(http::header::COOKIE).is_some();

    if !has_session {
        return MiddlewareResult::response(Redirect::to("/auth/login").into_response());
    }

    MiddlewareResult::next(req)
}
```

Middleware may freely inspect request metadata. If it consumes the request body, it must replace the body before returning `MiddlewareResult::next(req)`.

## How streaming works

When a route has `loading.{rs,html}`, the response is chunked:

```
[layout-open]
<div id="__nx_slot__"> …loading.html… </div>
                                       ← server awaits the page handler here
<template id="__nx_page__"> …page.html… </template>
<script>// ~200 bytes that swap the slot with the template's content </script>
[layout-close]
```

The browser sees the loading shell at TTFB (~250ms warm) and the page chunk arrives whenever the page handler resolves. No second HTTP request. Full architecture in [docs/streaming.md](docs/streaming.md).

## Run locally

```bash
cargo dev
# → http://localhost:3000
```

`cargo dev` runs the tiny `xtask` watcher, which starts `cargo run -p site`,
watches the framework and demo app sources, and restarts the server when Rust,
template, content, public asset, or env-file inputs change. The child command
gets `NEXTRS_SKIP_BUNDLE=0`, so local React page bundles are regenerated even
when a deploy config sets `NEXTRS_SKIP_BUNDLE=1`. The demo app wires
`tower-livereload` in debug builds, so the browser refreshes after the
restarted server is ready. That full-page live reload is the baseline dev
experience; React HMR/Fast Refresh is separate future work.

If you want the raw server without watching, run `cargo dev-once`.
The canonical setup for using this in other apps is documented in
[docs/local-dev-workflow.md](docs/local-dev-workflow.md).

Three demo routes — `/simple`, `/with-loading`, `/with-layout` — each progressively adding one more convention file. Each demo page lists its own source files inline so you can see exactly what's involved.

## Deploy to Vercel

The repo is set up to deploy as-is:

```bash
vercel deploy
```

Single binary at `api/index.rs` wraps the framework's axum router with `nextrs::vercel::StreamingVercelLayer` (a drop-in replacement for `vercel_runtime::axum::VercelLayer` that doesn't buffer `text/html` streaming responses). One catch-all rewrite in `vercel.json` (`/(.*)` → `/api/index`) routes everything to it. Static files live in `site/public/`; the workspace-root `build.rs` mirrors them to `public/` so Vercel serves them from the CDN edge cache.

Latency on a fresh preview deploy:

| Route | TTFB (warm p50) | Total | Notes |
|---|---|---|---|
| `/` | ~250ms | ~250ms | overview + root layout |
| `/simple` | ~220ms | ~220ms | no layout, no streaming |
| `/with-loading` | ~230ms | ~1080ms | loading streamed; page after 800ms simulated work |
| `/with-layout` | ~220ms | ~1090ms | nested layout + streamed loading + page |
| `/style.css` | ~145ms | ~145ms | CDN edge cache (`x-vercel-cache: HIT`) |

Cold start adds ~250-330ms above warm. Full deploy plan and details in [docs/vercel-deploy.md](docs/vercel-deploy.md).

## Project layout

Cargo workspace at root:

```
Cargo.toml         workspace + nextrs-deploy package (Vercel binary)
.cargo/config.toml cargo dev / cargo dev-once aliases
build.rs           emits the registry + mirrors site/public/ → public/ for Vercel
api/index.rs       Vercel entry point (22 lines) — generated registry + StreamingVercelLayer
xtask/             local dev watcher that restarts cargo run -p site
vercel.json        catch-all rewrite to /api/index
askama.toml        points askama at site/app/
public/            generated mirror of site/public/ (gitignored — CDN-served on Vercel)
nextrs/            framework crate (the lib)
  src/lib.rs
  src/conventions.rs    PageFn / LayoutFn / LoadingFn / MiddlewareFn types + static helpers
  src/discovery.rs      scans app/ → DiscoveredRoute list
  src/router.rs         build_router(_with_public)(registry) → axum::Router; streaming
  src/vercel.rs         StreamingVercelLayer  (feature-gated `vercel`)
  src/build.rs          codegen + sync_public_dir (feature-gated `build`)
site/              consumer crate — local-dev binary + the demo routes
  build.rs              runs nextrs::build::emit_registry from app/
  src/main.rs           include! the generated registry, serve via axum
  app/                  the convention tree
  public/               static assets (CSS, images) served at root URLs
  askama.toml           dirs = ["app"]
docs/
  streaming.md          architecture / how-to / verification
  vercel-deploy.md      deployment plan + research findings
  latency.md            latency breakdown + path to sub-100ms
```

User-facing files for adding a route: just files under `app/`. No mod declarations, no registry constructors. Codegen handles the wiring.

## Tests

```bash
cargo test --workspace --all-features
```

51 tests covering discovery (`.rs` + `.html` pairing, html-only, mixed nested, dynamic segments, API routes, middleware routes), conventions (static helpers and middleware helpers), router behavior (composition, layout-shell split, streaming chunk ordering, multi-frame body, **timing-based proof that the loading shell arrives before the page handler resolves**, middleware-before-loading redirects, nested middleware order, middleware request mutation, nested layouts under streaming, API methods, page+route coexistence), and codegen (skeleton structure, `.rs`-precedence, absolute path emission, middleware, route.rs methods).

## Status

- Single-binary Vercel deployment ✓
- HTML streaming through Fluid compute ✓
- Static assets via CDN ✓
- Nested layouts ✓
- Middleware before loading/page/API handlers ✓
- `.rs` and `.html` for every slot ✓
- Build-time codegen (no hand-wired `#[path]` mods or `RouteEntry` constructors) ✓

Future work lives in [ROADMAP.md](ROADMAP.md), including React HMR/Fast Refresh
and a first-class app scaffolder command.

Not yet:

- `error.{rs,html}` convention
- Per-route binaries on Vercel (single-binary is fine for now)
- Suspense-style nested streaming boundaries
