# Plan: Vercel deployment for nextrs

**Status:** Phase 0 done. Phase 2a hand-wired ahead of Phase 1 codegen — example deployed to Vercel and streaming verified end-to-end. Phase 1 (codegen) now the next thing to build.

## Goal

Make a nextrs app deployable to Vercel. The user runs `vercel deploy` (or `vc dev` for local emulation) on a project that contains an `app/` tree of convention files, and it just works — clean URLs, working routes, the loading→page streaming behavior preserved.

## Current state (relevant pieces)

- Workspace at root: `nextrs/` (lib) + `example/` (single Axum binary). One process, all routes registered with one `RouteRegistry` and served from one `Router`.
- Convention discovery exists (`nextrs/src/discovery.rs`) and produces `Vec<DiscoveredRoute>` with `.rs` and `.html` paths per slot.
- The example wires its routes by hand in `main.rs` using `#[path]` mod declarations. **There is no codegen yet.** Removing this boilerplate is the bulk of Phase 1.
- Streaming uses `axum::body::Body::from_stream` + `async-stream`. The framework owns the chunk format (`<div id="__nx_slot__">…loading…</div>` then `<template id="__nx_page__">…page…</template>` + ~200-byte swap script).

## Research findings (Phase 0)

### vercel_runtime supports axum natively

`vercel_runtime` v2 ships an `axum` feature flag that lets you hand a normal `axum::Router` to `vercel_runtime::run()`. This is documented in the [official Vercel Rust docs](https://vercel.com/docs/functions/runtimes/rust) and demonstrated in the [official Vercel axum template](https://github.com/vercel/examples/tree/main/rust/axum). Reference shape:

```toml
# Cargo.toml
[dependencies]
vercel_runtime = { version = "2", features = ["axum"] }
axum = "0.8"
tokio = { version = "1", features = ["full"] }
tower = "0.5"

[[bin]]
name = "index"
path = "api/index.rs"
```

```rust
// api/index.rs
use axum::{Router, routing::get};
use tower::ServiceBuilder;
use vercel_runtime::axum::VercelLayer;
use vercel_runtime::Error;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let router = Router::new().route("/", get(home));
    let app = ServiceBuilder::new()
        .layer(VercelLayer::new())
        .service(router);
    vercel_runtime::run(app).await
}
```

```json
// vercel.json
{
  "rewrites": [
    { "source": "/(.*)", "destination": "/api/index" }
  ]
}
```

The Vercel template uses literally this layout. **This is exactly Path A from the original plan, and it's the officially recommended approach.**

### Streaming is supported

Confirmed: Rust on Vercel runs on [Fluid compute](https://vercel.com/docs/fluid-compute), which supports HTTP response streaming. The official axum template demonstrates streaming via `vercel_runtime::axum::stream_response`:

```rust
use vercel_runtime::axum::stream_response;

async fn stream_example() -> impl IntoResponse {
    stream_response(|tx| async move {
        for i in 1..=5 {
            tx.send(Ok(Bytes::from(format!("chunk {}\n", i)))).await?;
        }
    })
}
```

**One open compat question:** our framework currently produces streams via `axum::body::Body::from_stream(...)`, returned as `Response<Body>`. Does that flow through `VercelLayer` correctly, or do we need to adapt to the `stream_response` helper? Both produce the same wire-level chunked response — likely just whether the layer's body conversion preserves chunk boundaries. **Test this in Phase 2 with a `curl --no-buffer` against a deployed function.** Worst case: change one helper.

### Single-binary deployment is the official path

The Vercel docs and example confirm that one `[[bin]]` + one catch-all rewrite to `/api/<name>` is the supported way to host an Axum app. Our app — already an Axum router — drops in directly. The `bundled_api` macro from the (now-deprecated) community runtime is no longer needed; the v2 `axum` feature does the same thing more cleanly.

### Cold start

Vercel docs state cold-start range of ~500–1000ms for Rust functions, materially better than Node and worse than Edge. With single-binary deployment we pay this *once* per warm cycle (not per route), which is the right tradeoff for a small-to-medium app. Not a Phase 0 blocker. Worth profiling once we have something deployed.

### Dynamic path segments

In the single-binary model, dynamic segments are Axum's responsibility — we already use `{id}` syntax in `Router::route`. Vercel's catch-all rewrite passes the full path through to our binary; Axum routes it. **No special Vercel-side handling needed for dynamic segments in the single-binary world.** When we eventually do per-route binaries (D1 future), we'll need to map `[id]` filename convention → Vercel `:id` rewrite syntax. Defer.

### Environment / build

- Vercel auto-detects `Cargo.toml` and runs the Rust builder.
- `.vercelignore` should exclude `target/**` except `target/release/` and `target/<triple>/release/**` for prebuilt deploys.
- `vc dev` runs functions locally with the Vercel emulator. Should work for our case but worth verifying once.

## Locked-in decisions

- **D1 — single binary now, design for per-route splitting later.** Start with the Vercel-recommended single-binary catch-all. Codegen layer is structured so a future `--target=vercel-bundled` can produce one binary per route without rewriting the whole codegen.
- **D2 — `build.rs` for codegen.** Same reasoning as before (proc-macros over filesystem trees are awkward and don't track file changes). Confirmed.

## Still-open decisions

- **D3 — vercel.json: regenerate or hand-edit?** Default: regenerate, with a clearly-marked custom-rewrites section the user can fill in if they need extra rewrites beyond the catch-all. Resolve in Phase 2.
- **D4 — Local-dev binary vs Vercel binary: same `main()` or different?** Two flavors:
  - **(a)** Codegen produces one `RouteRegistry`. Two thin `main()` files (one for `axum::serve` for local dev, one for `vercel_runtime::run` for Vercel) consume the same registry. **My preferred default** — keeps the registry as the single source of truth.
  - **(b)** The local-dev binary is the user's hand-written `main.rs`; Vercel codegen produces a separate `api/index.rs`. Less unified.

## Phases

### Phase 0 — Research ✅

Resolved (see findings above). One small follow-up: verify `axum::body::Body::from_stream` flows through `VercelLayer` cleanly. Defer to Phase 2's first deployment test.

### Phase 1 — Workspace codegen

Goal: replace the hand-wired `example/src/main.rs` with code generated from the `app/` tree. Same end-user behavior, but the user only writes convention files.

**Concrete before/after.** Today the user writes:

```rust
// example/src/main.rs (hand-wired)
#[path = "../app/layout.rs"] mod root_layout;
#[path = "../app/page.rs"] mod root_page;
#[path = "../app/simple/page.rs"] mod simple_page;
// ... 5 more #[path] declarations

#[tokio::main]
async fn main() {
    let mut registry = RouteRegistry::new();
    registry.add(RouteEntry {
        path: "/".to_string(),
        page: Some(Box::new(|req| Box::pin(root_page::render(req)))),
        layout: Some(Box::new(root_layout::render)),
        loading: None,
        methods: vec![],
    });
    // ... 3 more registry.add(...) calls
    let app = build_router(registry);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

After Phase 1:

```rust
// example/src/main.rs (generated registry, hand-written entry point)
include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

#[tokio::main]
async fn main() {
    let app = nextrs::router::build_router(generated_registry());
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

```rust
// example/build.rs (new file, ~5 lines)
fn main() {
    nextrs_build::emit_registry("app", "nextrs_routes.rs").unwrap();
}
```

The user owns `main.rs` (they pick the address, attach middleware like `tower-livereload`, etc.) and never touches the wiring. No `mod` declarations, no `RouteEntry` boilerplate.

**Concrete deliverables.**

1. New workspace member `nextrs-build/` — a small library crate consumed from `build.rs`. Public surface: `emit_registry(app_dir: &str, out_filename: &str) -> Result<(), Error>`.
2. Internally, `nextrs-build` calls `nextrs::discovery::discover_routes` (we already have this), then walks the result and emits Rust source code into `OUT_DIR/<out_filename>`.
3. Code emission rules per slot:
   - `page.rs` present → emit `#[path = "..."] mod page_<n>;` and `Some(Box::new(|req| Box::pin(page_<n>::render(req))))`
   - only `page.html` present → emit `Some(static_page(include_str!("...")))` (uses the `static_page` helper we already built)
   - same for `layout` and `loading`
4. The example crate adds `nextrs-build` as a `[build-dependencies]` and a `build.rs`, deletes its `#[path]` boilerplate, and `include!`s the generated file.
5. `cargo run -p nextrs-example` continues to work with identical behavior.

**Why this first.** Independent of Vercel — even if Vercel never happens, this is the right ergonomics. It also gives us the codegen infrastructure that Phase 2 builds on. And it lets us delete the `#[path]` boilerplate in `main.rs` immediately, which is genuine value to ship.

### Phase 2 — Vercel target

Add a Vercel build mode. Two sub-tasks:

**2a. Generate the Vercel entry point.** `nextrs-build` gains a function `emit_vercel_entry(app_dir, output_dir) -> Result<()>` that writes:
- `api/index.rs` — a `main()` that calls `vercel_runtime::run(VercelLayer + axum::Router)`, where the router is built from the same generated registry as Phase 1
- `vercel.json` — catch-all rewrite to `/api/index`
- (maybe) `.vercelignore` for prebuilt deploy size

**2b. Deploy and verify.** Push the example to a Vercel project, deploy, exercise all four routes (`/`, `/simple`, `/with-loading`, `/with-layout`). Specifically test:
- Static route renders correctly
- Streaming route (`/with-loading`) actually streams (`curl --no-buffer` shows multiple TCP chunks separated in time)
- Layout chain composes correctly across the network

If the streaming test fails, swap our `Body::from_stream` for `vercel_runtime::axum::stream_response` and retest.

### Phase 3 — DX polish

- A `nextrs` CLI binary (or a make-style script) that wraps `cargo build` for local vs Vercel targets
- README: getting started, conventions reference, deploy walkthrough
- Workflow file (`.github/workflows/ci.yml`) running `cargo test --workspace` and the example deploy on push
- Optional: dev-mode file watching on `app/` to re-run codegen + restart server

### Phase 4 — Per-route bundling (future)

Defer until single-binary is live and we have real cold-start data. When we do this:
- Codegen scans the registry, emits one `api/<route>.rs` per route, each importing only its own page/layout/loading files
- `vercel.json` rewrites map clean URLs to per-route functions
- Map filename `[id]` → Vercel rewrite `:id`
- `Cargo.toml` gets multiple `[[bin]]` entries

## Risks

| Risk | Severity | Mitigation |
|---|---|---|
| `Body::from_stream` doesn't flow through `VercelLayer` cleanly | Low | Adapter to `vercel_runtime::axum::stream_response` is mechanical. First deploy test surfaces this. |
| Cold start unacceptable for a particular use case | Low | Single-binary keeps it to once per warm cycle. Profile once deployed; consider lazy-init for heavy state. |
| Generated code is hard to debug | Low | `OUT_DIR` location is well-known; we also dump a copy under `target/nextrs/last_emit.rs` for inspection. |
| Vercel build cache misses force full rebuilds | Low | Cargo's incremental build + Vercel's filesystem cache should handle this. Only relevant if it bites us. |

## Out of scope

- Vercel Edge runtime (Wasm) — different target, different tradeoffs
- Other deploy targets (Fly, AWS Lambda direct, Cloud Run)
- Server actions / mutations beyond `route.rs`
- ISR / on-demand revalidation

## Phase 2a results — actually deployed (2026-05-05)

Hand-wired `api/index.rs` + `vercel.json` + made the workspace root a `[package]` with a `[[bin]]` entry. Deployed via `vercel deploy`. Project name: `nextrs` under `enzo-health` team. Production alias: `https://nextrs-umber.vercel.app` (public). Preview URLs are SSO-protected; access via `x-vercel-protection-bypass` header.

### The streaming bug we hit (and the fix)

`vercel_runtime::axum::VercelLayer` only treats a response as streaming when its `content-type` contains `text/event-stream` or `application/json`. We send `text/html`, so VercelLayer silently buffered the entire response via `axum::body::to_bytes(body, usize::MAX)` before returning. Symptom: TTFB == total response time on streaming routes; `curl --no-buffer --trace-time` showed a single `Recv data` event.

**Fix:** custom Tower service `StreamingVercelService` in `api/index.rs` that mirrors `VercelService`'s request-side conversion (collect Vercel body bytes → `axum::Body`) but unconditionally calls `StreamingUtils::create_stream_body(body)` for the response, bypassing the content-type gate. ~60 lines. No downside for non-streaming responses — they just arrive as one frame.

Worth filing upstream eventually: either an `always_stream` flag on `VercelLayer`, or extend `is_streaming_response` to recognize `text/html`.

### Latency measurements (preview, warm, p50)

| Route | TTFB | Total | Notes |
|---|---|---|---|
| `/` | ~250ms | ~250ms | overview + root layout |
| `/simple` | ~220ms | ~220ms | no layout, no streaming |
| `/with-loading` | ~230ms | ~1080ms | loading shell first; page after 800ms simulated work |
| `/with-layout` | ~220ms | ~1090ms | nested layout + streamed loading + page |

**Streaming chunk arrival on `/with-loading`:**
- T+0.000s: 1378-byte first frame (layout open + `<div id="__nx_slot__">…loading…</div>`)
- T+0.872s: 1728-byte second frame (`<template id="__nx_page__">…page…</template>` + swap script + layout close)

Cold start: first hit after deploy was 584ms TTFB; subsequent ~220-350ms, so cold added ~250-330ms. Tried to force a re-cold with 60s pause, function stayed warm — Fluid compute keeps Rust hot longer.

### What this changes for Phase 1 / 2b

- Phase 2a is done in spirit (hand-wired). Phase 1's job now includes generating `api/index.rs` AND `vercel.json` AND the workspace-root `[package]`/`[[bin]]` setup.
- The `StreamingVercelService` should probably move into the `nextrs` crate (gated by a `vercel` cargo feature) so codegen can `use nextrs::vercel::StreamingVercelService;` instead of inlining 60 lines.
- The duplication between `example/src/main.rs` (local) and `api/index.rs` (Vercel) is exactly what Phase 1 codegen deletes — both should be generated from the same `RouteRegistry`.

## Sources

- [Vercel Rust runtime docs](https://vercel.com/docs/functions/runtimes/rust)
- [Vercel axum example](https://github.com/vercel/examples/tree/main/rust/axum)
- [vercel_runtime crate (docs.rs)](https://docs.rs/vercel_runtime/latest/vercel_runtime/)
- [Rust on Vercel public-beta announcement](https://vercel.com/changelog/rust-runtime-now-in-public-beta-for-vercel-functions)
- [Fluid compute (streaming support)](https://vercel.com/docs/fluid-compute)
