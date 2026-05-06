# Manifest: nextrs

## Purpose

A **Next.js-like file-based routing framework for Rust**: folder-based routes, `page` / `layout` / `loading` conventions, and HTTP-level streaming that lets a route render its loading shell immediately and stream the real page when it's ready — without htmx, React, or any client-side framework.

The framework is built on Axum and Askama. Each segment under an `app/` directory is a route; convention files (`page.{rs,html}`, `layout.{rs,html}`, `loading.{rs,html}`, `route.rs`) compose the response. `.rs` files hold logic; `.html` files are static fallbacks. When a loading slot exists, the framework streams the layout's "before children" half, the loading shell in a slot div, then (after awaiting the page) a `<template>` plus a ~200-byte inline `<script>` that swaps the slot, then the layout's "after children" half.

## Layout

Cargo workspace at the repo root.

| Member | Purpose |
|---|---|
| `nextrs/` | The framework crate (library). Source at `nextrs/src/{lib,conventions,discovery,router}.rs`. |
| `example/` | A working consumer crate that demonstrates the conventions. Run with `cargo run -p nextrs-example` → http://localhost:3000. |

The framework is a normal Rust library. There is no codegen yet — the example wires its convention files into `main.rs` by hand using `#[path = "../app/.../{page,layout,loading}.rs"] mod ...;` declarations. Codegen will eventually produce these wirings automatically from a scan of the `app/` tree.

## Conventions

Each folder under `app/` is a route segment. The framework looks for these files at each segment:

| File | Purpose | Variants |
|---|---|---|
| `page.{rs,html}` | The main content for the route | `.rs` (async handler) preferred; `.html` (static) fallback |
| `layout.{rs,html}` | Wraps this segment's page and any nested segments | same precedence |
| `loading.{rs,html}` | Shown while the page resolves (triggers streaming) | same precedence |
| `route.rs` | API method handlers (POST/PUT/etc.) — `.rs` only |

Layout templates rendered through Askama must use `{{ children|safe }}` (not `{{ children }}`) so the framework's internal content marker passes through unescaped. Static layouts loaded via `nextrs::conventions::static_layout` accept either `{{children}}` or `{{ children }}` and do literal substitution (no askama escaping).

Dynamic URL segments use `[param]` directory naming (e.g. `app/users/[id]/page.rs` → `/users/{id}` in Axum's path syntax).

## Where to look

| Area | File |
|---|---|
| Slot/file discovery | `nextrs/src/discovery.rs` — scans `app/` and produces `DiscoveredRoute { page, layout, loading, route }` where each slot tracks both `.rs` and `.html` paths |
| Route handler types | `nextrs/src/conventions.rs` — `PageFn`, `LayoutFn`, `LoadingFn`, `RouteFn`; static helpers `static_page`, `static_layout`, `static_loading` |
| Routing + streaming | `nextrs/src/router.rs` — `build_router(registry) -> axum::Router`. Composes layouts around a content marker, splits on the marker, streams loading-then-page when a loading slot is present |
| Progressive demo | `example/app/{simple, with-loading, with-layout}/` — three routes that progressively add `loading.{rs,html}` and `layout.{rs,html}`. The home page (`example/app/page.html`) is an overview with links and a per-route file listing |
| Example wiring | `example/src/main.rs` — `#[path]` mod declarations and `RouteRegistry` setup |
| Askama config | `example/askama.toml` — points the askama template directory at `app/` so templates can sit next to their `.rs` siblings |

## Tests

`cargo test --workspace` (32 tests):

- Discovery: `.rs` + `.html` pairing, html-only segments, mixed nested, dynamic segments, API routes
- Conventions: static helpers (`static_page`, `static_layout` with both `{{children}}` and `{{ children }}` forms, `static_loading`)
- Router: synchronous render, layout composition (1 / 3 levels deep), mixed static/dynamic layouts, layout-shell split-on-marker, streaming chunk ordering, multi-frame body, nested layouts under streaming, API methods, page+route on same path

## Non-goals

- **No codegen yet.** The example wires routes by hand. Codegen that turns a `DiscoveredRoute` list into a `RouteRegistry` is the next major step.
- **No client-side framework.** No htmx, no React, no JS bundle. The loading→page swap is a single inline script the framework emits; nothing else runs in the browser.
- **Not pinning a public API.** Types and helpers may move as conventions harden.

## Roadmap (rough)

1. Codegen: convert a `DiscoveredRoute` tree into a generated `RouteRegistry` so users only write convention files, not `main.rs`.
2. `error.{rs,html}` segment convention.
3. Dev-server ergonomics (askama proc-macros don't track `.html` deps, so editing a template currently requires `touch`-ing the consuming `.rs` file or `cargo clean`).
4. Static asset serving conventions.
