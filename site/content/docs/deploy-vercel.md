+++
title = "Deploy to Vercel"
description = "Run the shared Rust app as one Vercel function, regenerate frontend assets during the build, and preserve streaming"
section = "Deploy"
order = 10
+++

A nextrs app deploys to Vercel as one Rust binary behind a catch-all rewrite.
Static files and generated browser bundles are served from `public/`; dynamic
requests reach the Axum router through a thin Vercel adapter.

The scaffold includes this target by default. It also defaults to prebuilt
deploys with git auto-builds disabled, because local Rust builds avoid the
cloud build queue. The `vercel.json` build remains self-contained if you
choose to re-enable cloud builds.

## One application, two process adapters

Application construction belongs in `src/app.rs`:

```rust
// src/app.rs
include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

pub fn app() -> axum::Router {
    let public = concat!(env!("CARGO_MANIFEST_DIR"), "/public");
    nextrs::router::build_router_with_public(generated_registry(), public)
        .merge(nextrs::openapi::spec_router(generated_openapi()))
}
```

`src/main.rs` starts that app locally. Vercel currently requires its Rust
function at `api/index.rs`, so the scaffold supplies a second, deliberately
thin process adapter:

```rust
// api/index.rs -- do not put application logic here
use nextrs::vercel::StreamingVercelLayer;
use tower::ServiceBuilder;

#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    let app = ServiceBuilder::new()
        .layer(StreamingVercelLayer::new())
        .service(my_app::app());

    vercel_runtime::run(app).await
}
```

Both processes call the same `app()`, so routes and application layers cannot
drift.

The corresponding Cargo targets and runtime dependencies are:

```toml
[lib]
path = "src/app.rs"

[[bin]]
name = "my-app"
path = "src/main.rs"

[[bin]]
name = "index"
path = "api/index.rs"

[dependencies]
nextrs = { version = "0.5", features = ["vercel"] }
tower = "0.5"
vercel_runtime = { version = "2", features = ["axum"] }
```

## Vercel configuration

The generated `vercel.json` installs and builds from the application root:

```json
{
  "$schema": "https://openapi.vercel.sh/vercel.json",
  "installCommand": "npm ci",
  "buildCommand": "npm run client:prepare && cargo build --release --bin index && npm run client:build",
  "functions": {
    "api/index.rs": { "runtime": "vercel-rust@4.0.11" }
  },
  "headers": [
    {
      "source": "/dist/(.*)",
      "headers": [
        { "key": "Cache-Control", "value": "public, max-age=31536000, immutable" }
      ]
    }
  ],
  "rewrites": [
    { "source": "/(.*)", "destination": "/api/index" }
  ],
  "git": { "deploymentEnabled": false }
}
```

The `functions` block is mandatory because `vercel-rust` is a community
runtime and is not selected automatically. The catch-all passes the original
path to Axum, including dynamic segments.

The build sequence is intentional:

1. `npm ci` installs root dependencies and links `.nextrs/client`.
2. `client:prepare` dumps `.nextrs/openapi.json` and generates both client
   surfaces.
3. the release Cargo build discovers routes and bundles React pages into
   `public/dist` while compiling the Vercel function;
4. `client:build` emits the package's JavaScript and `.d.ts` files.

Do not set `NEXTRS_SKIP_BUNDLE` for a normal deploy. `public/dist`,
`.nextrs/openapi.json`, and the entire `.nextrs/client` package are disposable
build output. The build materializes the ignored workspace package from its
tracked framework template, then verifies its JavaScript and declarations.
Dependencies are installed once at the app root, never inside the generated
client.

`rust-toolchain.toml` pins the toolchain used by the TSX bundler dependency
tree. The current scaffold uses Rust 1.96:

```toml
[toolchain]
channel = "1.96.0"
```

If the deploy root has `.cargo/config.toml`, keep an explicit `[build]` table
alongside the `cargo dev` alias. Some `vercel-rust` versions read
`config.build.target` during setup:

```toml
[alias]
dev = "nextrs dev --bin my-app"

[build]
```

## Deploy

The default scaffold disables git-triggered builds. Use its prebuilt script:

```bash
scripts/deploy-prebuilt.sh           # production
scripts/deploy-prebuilt.sh --preview # preview
```

This runs `vercel build` on your machine and uploads its Build Output. See
[Build Locally, Ship Artifacts](/docs/deploy-prebuilt) for setup.

If you prefer Vercel cloud builds, delete the `git.deploymentEnabled: false`
setting (or enable it in project settings) and push normally. The same root
install/build commands regenerate everything in the cloud; no prebuilt bundle
needs to be checked into git.

## Streaming through the adapter

The stock `vercel_runtime::axum::VercelLayer` only streams a limited set of
content types. nextrs streams `text/html`, so the stock layer can buffer the
loading shell until the full page is ready.

`nextrs::vercel::StreamingVercelLayer` streams the Axum response body without
that content-type restriction. Non-streaming responses continue to work. If a
deployed loading route has `TTFB` approximately equal to total time, first
confirm that `api/index.rs` installs this layer.

## Background work after the response

Detached `tokio::spawn` work is unsafe in a serverless invocation because the
instance can freeze after sending the response. Use `nextrs::WaitUntil`:

```rust
use nextrs::WaitUntil;

pub async fn post(wait: WaitUntil, Json(req): Json<AddTodoRequest>) -> Json<Todo> {
    let todo = add(req.title).await;
    let audit = todo.clone();
    wait.wait_until(async move {
        audit_log(&audit).await;
    });
    Json(todo)
}
```

Behind `StreamingVercelLayer`, the future is registered with the runtime's
shutdown drain. Local and container execution fall back to spawning it. Log
failures inside the future because its output is discarded.

## Static assets

Vercel serves root-level `public/` files before applying the catch-all rewrite.
That includes user assets such as `/logo.svg` and the content-addressed files
under `/dist/`. The generated immutable cache header is safe for `/dist/`
because changing content produces a different filename.

## Verify after deploying

```bash
curl -o /dev/null \
  -w "TTFB=%{time_starttransfer}s total=%{time_total}s\n" \
  https://your-deployment.vercel.app/slow
```

For a route with a delayed server prefetch, `TTFB` should be meaningfully less
than total time. Preview URLs protected by Vercel authentication require the
corresponding protection-bypass header.

## Removing Vercel support

If Vercel is not a target, remove the whole adapter surface together:

- `api/index.rs`;
- the `index` Cargo target;
- `vercel_runtime`, `tower` if otherwise unused, and the nextrs `vercel`
  feature if otherwise unused;
- `vercel.json` and the prebuilt deployment script.

Keep `src/app.rs`, `src/main.rs`, and `build.rs`: they are the shared
application, local process, and Rust build infrastructure, not Vercel code.
