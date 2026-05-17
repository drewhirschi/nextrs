# nextrs

A Next.js-style routing framework for Rust. File-based routes, `page` / `layout` / `loading` conventions, HTTP-level streaming for the loading shell — no client-side framework, no htmx, no React.

Built on Axum and Askama. Deploys to Vercel as a single Rust function with the loading→page swap streamed over chunked transfer encoding.

## Quick look

```
site/app/
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
| `route.rs` | API method handlers (POST/PUT/etc.) | `.rs` only |

`.rs` files are Rust handlers (typically askama templates with logic). `.html` files are static fallbacks. When both exist for a slot, `.rs` wins.

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
cargo run -p site
# → http://localhost:3000
```

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
build.rs           emits the registry + mirrors site/public/ → public/ for Vercel
api/index.rs       Vercel entry point (22 lines) — generated registry + StreamingVercelLayer
vercel.json        catch-all rewrite to /api/index
askama.toml        points askama at site/app/
public/            generated mirror of site/public/ (gitignored — CDN-served on Vercel)
nextrs/            framework crate (the lib)
  src/lib.rs
  src/conventions.rs    PageFn / LayoutFn / LoadingFn types + static helpers
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

37 tests covering discovery (`.rs` + `.html` pairing, html-only, mixed nested, dynamic segments, API routes), conventions (static helpers), router behavior (composition, layout-shell split, streaming chunk ordering, multi-frame body, **timing-based proof that the loading shell arrives before the page handler resolves**, nested layouts under streaming, API methods, page+route coexistence), and codegen (skeleton structure, `.rs`-precedence, absolute path emission).

## Status

- Single-binary Vercel deployment ✓
- HTML streaming through Fluid compute ✓
- Static assets via CDN ✓
- Nested layouts ✓
- `.rs` and `.html` for every slot ✓
- Build-time codegen (no hand-wired `#[path]` mods or `RouteEntry` constructors) ✓

Not yet:

- `error.{rs,html}` convention
- Per-route binaries on Vercel (single-binary is fine for now)
- Suspense-style nested streaming boundaries
- Dev-server file watching with auto-rebuild
- `route.rs` codegen (currently emits `methods: vec![]` for every route)
