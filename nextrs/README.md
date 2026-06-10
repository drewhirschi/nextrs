# nextrs

A Next.js-style routing framework for Rust, built on [Axum](https://docs.rs/axum) and [Askama](https://docs.rs/askama).

File-based routes with `page` / `layout` / `loading` / `middleware` / `route` conventions, HTTP-level streaming for the loading shell, an optional typed-client pipeline (OpenAPI → React Query hooks), and optional React `page.tsx` pages whose data the server can seed into the client cache. Deploys to Vercel as a single Rust function, or runs anywhere as a normal Axum app.

```text
app/
├── layout.{rs,html}      root layout, wraps every route
├── page.{rs,html,tsx}    /              — .rs/.html server-rendered, or .tsx React
├── middleware.rs         request guard, composed root-to-leaf
├── api/ping/route.rs     /api/ping      — typed HTTP handlers (get/post/…)
└── dashboard/
    ├── loading.{rs,html} streamed shell shown while the page resolves
    └── page.{rs,html}    /dashboard
```

Each directory is a URL segment; each filename is a convention slot. `.rs` wins when both `.rs` and `.html` exist. Adding a route is just adding a file — a build-time codegen step (the `build` feature, called from your `build.rs`) discovers the tree and wires the router, with no hand-written `mod` declarations or registry constructors.

## Quick start

```toml
[dependencies]
nextrs = "0.1"
axum = "0.8"
tokio = { version = "1", features = ["full"] }
askama = "0.15"

[build-dependencies]
nextrs = { version = "0.1", features = ["build"] }
```

```rust,ignore
// build.rs
fn main() {
    nextrs::build::emit_registry("app", "src/main.rs", "nextrs_routes.rs").unwrap();
}
```

```rust,ignore
// src/main.rs
include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

#[tokio::main]
async fn main() {
    let app = nextrs::router::build_router(generated_registry());
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

## Features

- **`build`** — the build-script codegen (`nextrs::build`, `nextrs::docs`): route discovery, the generated registry, and a markdown→docs+llms.txt pipeline. Depend on it under `[build-dependencies]` only.
- **`tsx`** — build-time bundling of React `page.tsx` entries via [rolldown](https://crates.io/crates/rolldown). Heavy build-dependency tree; enable only if you use `.tsx` pages.
- **`vercel`** — `nextrs::vercel::StreamingVercelLayer`, a drop-in replacement for the upstream Vercel layer that preserves `text/html` streaming.

## Streaming

A route with a `loading` slot streams the shell first and swaps in the page when its handler resolves — one HTTP response, no client framework:

```text
[layout-open]
<div id="__nx_slot__"> …loading… </div>      ← arrives at TTFB
<template id="__nx_page__"> …page… </template> ← arrives when the handler resolves
<script> /* ~200 bytes: swap slot ← template */ </script>
[layout-close]
```

## Status

Pre-1.0 — the API will change. See the [repository](https://github.com/drewhirschi/nextrs) for the full docs, examples (including a React + server-seeded-cache todo app), and design notes.

## License

Apache-2.0
