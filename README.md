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
| `page.{rs,html,tsx}` | The main content | `.rs` (async handler) or `.html` (static) for Rust/Askama; `.tsx` for a React client page (`tsx` feature) |
| `layout.{rs,html,tsx}` | Wraps the segment's page and any nested segments | same |
| `loading.{rs,html,tsx}` | Triggers streaming — shown while the page resolves | same |
| `middleware.rs` | Runs before page rendering, loading streaming, and API handlers | `.rs` only |
| `route.rs` | API method handlers (POST/PUT/etc.) | `.rs` only |
| `prefetch.rs` | Server data for a sibling `page.tsx` — seeds the React Query cache (legacy name `props.rs` still works) | `.rs` only |

`.rs` files are Rust handlers (typically askama templates with logic). `.html` files are static fallbacks; when both exist for a slot, `.rs` wins. `.tsx` files are React client pages — bundled by an embedded rolldown bundler (behind the `tsx` cargo feature) into content-addressed `/dist/<slug>-<hash>.js` assets and mounted into `<div id="__nx_root__">` under a TanStack React Query provider.

A React route pairs a `.tsx` page with a `prefetch.rs` for server-seeded data (see [`examples/react-todos`](examples/react-todos)):

```
app/
├── layout.{rs,html}            ← Rust/Askama shell around the React tree
├── page.tsx                    ← / — React client page, bundled to /dist/<slug>-<hash>.js
└── prefetch.rs                    ← async fn prefetch(req) -> nextrs::QuerySeed (cache seed)
```

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
cargo install --path crates/cargo-nextrs-dev   # once — installs the `cargo nextrs-dev` runner
cd site
cargo dev
# → http://localhost:3000
```

The `site/` app is self-contained: you `cd site` and do everything from there.
`cargo dev` (aliased in `site/.cargo/config.toml` to `nextrs-dev --bin site`)
runs the shipped `cargo-nextrs-dev` watcher — the exact tool apps generated by
`create-nextrs-app` use — so the framework dogfoods its own runner. It starts
the server, watches the app's Rust, template, content, public asset, and
env-file inputs, and restarts on change. The site's React landing (`app/page.tsx`)
is bundled via the embedded rolldown path (`tsx` feature); Node is only needed
to regenerate the typed client in `site/client` (orval). The app wires
`tower-livereload` in debug builds, so the browser refreshes after the restarted
server is ready. That full-page live reload is the baseline dev experience;
React HMR/Fast Refresh is separate future work.

The canonical setup for using this runner in other apps is documented in
[docs/local-dev-workflow.md](docs/local-dev-workflow.md).

Three demo routes — `/simple`, `/with-loading`, `/with-layout` — each progressively adding one more convention file. Each demo page lists its own source files inline so you can see exactly what's involved.

## Deploy to Vercel

The `site/` app deploys as-is (Vercel project Root Directory = `site`):

```bash
cd site
vercel deploy
```

`site` is a self-contained deployable. Its `index` binary at `site/api/index.rs` wraps the framework's axum router with `nextrs::vercel::StreamingVercelLayer` (a drop-in replacement for `vercel_runtime::axum::VercelLayer` that doesn't buffer `text/html` streaming responses). One catch-all rewrite in `site/vercel.json` (`/(.*)` → `/api/index`) routes everything to it. Vercel installs the client dependencies and regenerates `public/dist/` during every build. The emitted JavaScript and stylesheet use content-addressed URLs, and the deployment config serves `/dist/*` with a one-year immutable cache policy.

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

Cargo workspace at root (a pure virtual manifest — no root package):

```
Cargo.toml         workspace members + patch + profiles
crates/            the framework + tooling crates
 nextrs/           framework crate (the lib)
  src/lib.rs
  src/conventions.rs    PageFn / LayoutFn / LoadingFn / MiddlewareFn types + static helpers
  src/discovery.rs      scans app/ → DiscoveredRoute list (page/layout/loading {rs,html,tsx}, prefetch.rs)
  src/router.rs         build_router(_with_public/_with_prefetch)(registry) → axum::Router; streaming
  src/prefetch.rs       Speculation Rules navigation prefetch (PrefetchConfig → <script>)
  src/seed.rs           QuerySeed / SeedEntry / seed_key — prefetch.rs React Query cache seeding
  src/openapi.rs        spec_router — serves the build-time OpenAPI doc
  src/vercel.rs         StreamingVercelLayer  (feature-gated `vercel`)
  src/build.rs          codegen + sync_public_dir (feature-gated `build`)
  src/docs.rs           markdown docs + llms.txt pipeline (feature-gated `build`)
  src/bundle.rs         embedded rolldown bundling of .tsx pages → /dist (feature-gated `tsx`)
 nextrs-macros/    proc-macro crate (paired with nextrs)
 create-nextrs-app/ React-first app scaffolder (`create-nextrs-app`)
 cargo-nextrs-dev/  the `cargo nextrs-dev` watcher generated apps (and site) use
examples/react-todos  React .tsx + prefetch.rs + typed client demo app
site/              self-contained docs/demo app — dev binary + Vercel deploy
  src/main.rs           dev binary: include! the generated registry, serve via axum
  api/index.rs          Vercel entry (`index` bin) — same registry + StreamingVercelLayer
  build.rs              nextrs::build::emit_registry + emit_docs + bundle_pages from app/
  app/                  the convention tree (incl. page.tsx React landing)
  public/               static assets (CSS, images) + generated dist/ bundle
  style/                Tailwind/DaisyUI build → public/style.css
  vercel.json           client/build commands + immutable dist headers + catch-all rewrite
  .cargo/config.toml    `cargo dev` alias → nextrs-dev --bin site
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

~121 tests (across `nextrs` + `nextrs-macros`) covering discovery (`.rs` + `.html` + `.tsx` slots, prefetch.rs, html-only, mixed nested, dynamic segments, API routes, middleware routes), conventions (static helpers and middleware helpers), router behavior (composition, layout-shell split, streaming chunk ordering, multi-frame body, **timing-based proof that the loading shell arrives before the page handler resolves**, middleware-before-loading redirects, nested middleware order, middleware request mutation, nested layouts under streaming, API methods, page+route coexistence), codegen (skeleton structure, `.rs`-precedence, absolute path emission, middleware, route.rs methods), `prefetch.rs` seeding (`seed_key` shapes, script-tag escaping, entry ordering), and Speculation Rules prefetch config injection.

## Status

- Single-binary Vercel deployment ✓
- HTML streaming through Fluid compute ✓
- Static assets via CDN ✓
- Nested layouts ✓
- Middleware before loading/page/API handlers ✓
- `.rs`, `.html`, and `.tsx` for the page/layout/loading slots ✓
- React client pages — `.tsx` bundled by the embedded rolldown bundler (`tsx` feature), mounted under a TanStack React Query provider ✓
- `prefetch.rs` server seeding into the React Query cache (`QuerySeed` / `seed_key`) ✓
- Typed React Query client generated from the build-time OpenAPI doc (orval) ✓
- Native Speculation Rules navigation prefetch ✓
- Build-time codegen (no hand-wired `#[path]` mods or `RouteEntry` constructors) ✓
- `create-nextrs-app` scaffolder for React-first apps ✓

Future work lives in [ROADMAP.md](ROADMAP.md), including React HMR/Fast Refresh.

Not yet:

- `error.{rs,html}` convention
- Per-route binaries on Vercel (single-binary is fine for now)
- Suspense-style nested streaming boundaries
