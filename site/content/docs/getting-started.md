+++
title = "Getting Started"
description = "Set up a nextrs app: the app/ tree, build-time codegen, and the dev loop"
section = "Guides"
order = 1
+++

nextrs is a Next.js-style routing framework for Rust. You write convention files (`page.tsx`, `page.rs`, `layout.rs`, `loading.html`, `middleware.rs`, `prefetch.rs`, `route.rs`) in an `app/` directory; a build step discovers them and wires the router. Two rendering models coexist: Rust/HTML pages rendered with Askama (streamed when a route has a loading state), and React `.tsx` pages bundled to the client with their server data seeded from a `prefetch.rs` sibling.

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
nextrs = "0.3"
axum = "0.8"
tokio = { version = "1", features = ["full"] }
askama = "0.15"

[build-dependencies]
nextrs = { version = "0.3", features = ["build"] }
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

## React pages

A route can render a React component instead. A `page.tsx` (plus optional `layout.tsx` / `loading.tsx`) is bundled to `public/dist/<slug>.js` at build time by an embedded [rolldown](https://rolldown.rs) bundler — there is no separate Node build step — gated behind the `tsx` cargo feature on the build-dependency:

```toml
[build-dependencies]
nextrs = { version = "0.3", features = ["build", "tsx"] }
```

`build.rs` adds a bundle step alongside `emit_registry`:

```rust
nextrs::bundle::bundle_pages(&nextrs::bundle::BundleConfig {
    app_dir: "app",
    client_dir: "client",
    client_alias: "@mysite/client",
    public_dist: "public/dist",
    ..Default::default()
})
.expect("nextrs::bundle::bundle_pages failed");
```

Each page mounts into `<div id="__nx_root__">` under a TanStack `<QueryClientProvider>`. A sibling `client/` package holds the typed React Query client — generated from the app's OpenAPI spec with [orval](https://orval.dev) and imported through an alias like `@mysite/client`, so calling a Rust `route.rs` handler is a typed `useGet…` hook.

### Server data with `prefetch.rs`

To warm the React Query cache on the server, drop a `prefetch.rs` next to a `page.tsx`. It exports `pub async fn prefetch(req) -> nextrs::QuerySeed`:

```rust
pub async fn prefetch(_req: http::Request<axum::body::Body>) -> nextrs::QuerySeed {
    nextrs::QuerySeed::new()
        .seed(async {
            nextrs::SeedEntry {
                key: nextrs::seed_key("/slow/message", None),
                data: nextrs::serde_json::json!({ "message": "Loaded from Rust." }),
            }
        })
        .await
}
```

The framework streams the entries as a JSON `<script id="__nx_seeds__">` tag and the client loads them into the React Query cache before mount, so the page renders with the data already in place. Keys built with `seed_key` match the generated client's query keys exactly, so a seeded entry behaves like a fetched one (mutations and `invalidateQueries` reach it the same way).

`create-nextrs-app` scaffolds this whole track for you — `app/page.tsx`, a `/slow` route with `prefetch.rs` + `loading.tsx`, an `/api/ping` handler, the `client/` orval package, and an `AGENTS.md` contract for coding agents — so it's the fastest way to start a React-first app. Bringing an existing app instead? `create-nextrs-app --adopt` grafts the same skeleton into a non-empty repo without overwriting anything — see [Porting an Existing App](/docs/porting).

## The dev loop

Generated apps use `cargo-nextrs-dev`, a file watcher that rebuilds and restarts the server when anything relevant changes (source, templates, `app/` files, assets, `.env`). Install it once, then run `cargo dev` — the generated `.cargo/config.toml` aliases `dev` to `nextrs-dev --bin <crate>`:

```bash
cargo install cargo-nextrs-dev
cargo dev
```

It debounces changes, SIGTERMs the running server cleanly, rebuilds with `cargo build --bin <crate>`, and restarts — without interrupting an in-progress Cargo build. Combined with `tower-livereload`, the browser refreshes itself after the rebuild. This is full-page live reload, not React HMR.

## Where to go next

- [Routing Conventions](/docs/conventions) — every file type the framework understands.
- [Streaming](/docs/streaming) — how `loading` slots stream the shell before the page resolves.
- [Deploy to Vercel](/docs/deploy-vercel) or [Deploy with Docker](/docs/deploy-docker).
