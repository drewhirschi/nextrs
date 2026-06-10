+++
title = "Getting Started"
description = "Set up a nextrs app: the app/ tree, build-time codegen, and the dev loop"
section = "Guides"
order = 1
+++

nextrs is a Next.js-style routing framework for Rust. You write convention files (`page.rs`, `layout.rs`, `loading.html`, `middleware.rs`, `route.rs`) in an `app/` directory; a build step discovers them and wires the router. No client-side framework — pages are server-rendered HTML, streamed when a route has a loading state.

## The pieces

A nextrs app is a normal Rust binary crate plus three things:

```
mysite/
├── Cargo.toml          # depends on nextrs; build-dep on nextrs with "build" feature
├── build.rs            # one call: emit_registry
├── askama.toml         # points Askama at app/ for templates
├── app/                # your routes (the convention tree)
│   ├── layout.rs       # root layout (+ layout.html Askama template)
│   ├── page.rs         # /
│   └── hello/
│       └── page.html   # /hello — static HTML needs no Rust at all
├── public/             # static assets, served at the root URL path
└── src/
    └── main.rs         # ~15 lines: include the registry, serve it
```

`Cargo.toml`:

```toml
[dependencies]
nextrs = "0.1"
axum = "0.8"
tokio = { version = "1", features = ["full"] }
askama = "0.15"

[build-dependencies]
nextrs = { version = "0.1", features = ["build"] }
```

`build.rs`:

```rust
fn main() {
    nextrs::build::emit_registry("app", "src/main.rs", "nextrs_routes.rs")
        .expect("nextrs::build::emit_registry failed");
}
```

`emit_registry` scans `app/`, and writes a generated `generated_registry()` function into `$OUT_DIR`. It also tells cargo to rerun whenever anything under `app/` changes, so adding a file is enough — no manual wiring, ever. (A copy of the generated code is dumped to `target/nextrs/` if you want to read it.)

`src/main.rs`:

```rust
include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

#[tokio::main]
async fn main() {
    let app = nextrs::router::build_router_with_public(
        generated_registry(),
        concat!(env!("CARGO_MANIFEST_DIR"), "/public"),
    );
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

You own `main.rs` — pick the address, attach tower layers (the demo site adds `tower-livereload` in debug builds), read env vars. The framework only owns the router.

`askama.toml`:

```toml
[general]
dirs = ["app"]
```

## Your first page

`app/page.rs` plus an Askama template `app/page.html`:

```rust
use askama::Template;

#[derive(Template)]
#[template(path = "page.html")]
pub struct HomePage;

pub async fn render(_req: http::Request<axum::body::Body>) -> String {
    HomePage.render().unwrap()
}
```

Pages are async functions from a request to an HTML string. They can await anything — database calls, upstream APIs — and read headers, the URI, and extensions set by middleware from the request. If a page doesn't need Rust at all, skip the `.rs` file and write just `page.html`; the build step serves it statically.

Run it:

```bash
cargo run
# Listening on 0.0.0.0:3000
```

## The dev loop

The repo ships a file watcher that restarts the server when anything relevant changes (source, templates, content, assets):

```bash
cargo run --bin nextrs-dev
```

It polls for changes, debounces, SIGTERMs the server cleanly, and restarts. Combined with `tower-livereload`, the browser refreshes itself after the rebuild.

## Where to go next

- [Routing Conventions](/docs/conventions) — every file type the framework understands.
- [Streaming](/docs/streaming) — how `loading` slots stream the shell before the page resolves.
- [Deploy to Vercel](/docs/deploy-vercel) or [Deploy with Docker](/docs/deploy-docker).
