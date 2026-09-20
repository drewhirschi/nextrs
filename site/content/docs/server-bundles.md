+++
title = "Server Bundles"
description = "Give expensive routes their own Rust executable and deployed function using bundle.toml"
section = "Guides"
order = 14
+++

Server bundles keep expensive dependencies and runtime files out of ordinary
functions. Each bundle is compiled independently and deployed as one function.
Public URLs and generated browser clients stay the same.

This feature currently requires the framework and CLI from this repository's
source; it is not part of the published 0.6.1 framework / 0.3.0 CLI. Until the
next coordinated release, run the checkout's CLI, for example:

```bash
cargo run -p cargo-nextrs --bin nextrs -- bundles plan --root examples/react-todos
```

The shorter `nextrs` commands below assume that source-built CLI is on your PATH.

## Assign a route or subtree

Put a `bundle.toml` beside your route conventions:

```text
app/
  page.tsx                         # default bundle
  api/
    documents/
      bundle.toml                  # documents bundle
      route.rs
      [id]/route.rs                # inherits documents
      status/
        bundle.toml                # can override the parent
        route.rs
```

```toml
# app/api/documents/bundle.toml
bundle = "documents"
```

Assignments inherit down the directory tree. The nearest named assignment wins;
without an assignment, an endpoint belongs to `default`. All HTTP methods in a
`route.rs` stay together. A colocated page also belongs to the same bundle.
Directory names and public URLs do not change.

To return a nested route to the default function:

```toml
# app/api/documents/status/bundle.toml
bundle = "default"
```

## Isolate one endpoint

```toml
# app/api/video/render/bundle.toml
isolate = true
features = ["video"]
assets = ["resources/video"]
```

This creates a stable, private bundle name for this endpoint. It includes its
methods and required ancestor conventions. Unlike a named assignment,
`isolate = true` applies only to the colocated endpoint; descendants continue to
inherit the nearest named ancestor. Use a named bundle to group a subtree.

Names use lowercase letters, digits, and hyphens. The `route-` prefix is reserved
for generated isolated names. A declaration without an endpoint is an error.

## Keep dependencies separate

A routing tag alone cannot remove an unconditional Cargo dependency or shared
initialization code. Make heavy dependencies optional:

```toml
# Cargo.toml
[features]
default = ["documents"] # convenient for ordinary local development
documents = ["dep:pdf-engine"]

[dependencies]
pdf-engine = { version = "1", optional = true } # replace with your actual engine
```

Declare features and private runtime files for named bundles centrally:

```toml
# nextrs.toml (alongside the existing [app] and [vercel] tables)
[deployment]
features = [] # Cargo features common to every bundle

[bundles.documents]
features = ["documents"]
assets = ["resources/fonts", "resources/templates/invoice.html"]
```

Assets are explicit files or directories relative to the application root; globs
and symlinks are not supported. They retain their relative paths inside the
function. For example, the handler opens `resources/templates/invoice.html`.
Keep private runtime resources outside `public/`, which is served by the CDN.

Each bundle gets a separate Cargo invocation with `--no-default-features` and
only its declared features. The framework excludes other endpoints' Rust modules
before compiling. Gate heavy modules in `src/` and their initialization with
`#[cfg(feature = "documents")]` too: shared application code is compiled in every
bundle. Ordinary Cargo builds do not select a bundle and still contain all routes.

## Inspect and build

```bash
nextrs bundles plan
nextrs bundles build                     # native release binaries
nextrs bundles build --dev               # native debug binaries for testing
nextrs bundles build --bin my-server     # choose the process adapter
nextrs bundles build --vercel            # Linux executable functions + routing
nextrs bundles build --vercel --output DIR   # write the finished output elsewhere
nextrs bundles verify                    # check an output against the packaging rules
```

Every command accepts `--root path/to/app`. Native builds use the package's
`default-run` binary, falling back to the package name. Vercel builds use `index`,
require `cargo-zigbuild` and Zig, and target x86-64 Linux.

Native output lives in `.nextrs/bundles/<name>/executable`. Run each process from
its bundle directory so relative runtime asset paths resolve. Configure
`NEXTRS_PUBLIC_DIR` if serving public assets from a separate directory. Use the
manifest's route ownership to configure your self-hosted reverse proxy.

The builder checks a receipt from the application build script against the route
plan and refuses to package an older framework that ignored bundle selection.

Vercel output lives in `.vercel/output/functions/__nextrs_functions/<name>.func`.
The output also contains `routing.json` (the generated dispatch rules),
`bundle-manifest.json` (ownership/features/assets) and
`bundle-artifacts.json` (executable sizes and declared inputs). These files are
build reports, not public assets. Frontend client generation must run before a
standalone bundle build if the app uses a generated client.

`--output` and `bundles verify` exist for apps that cannot compile on the
deploying machine — typically because a bundle links native system libraries
and must build in a container. `bundles verify` applies the same packaging
rules `nextrs deploy` enforces on a `[build] command`; see
[Custom Build Command](/docs/custom-build).

## Deploy and route requests

```bash
nextrs deploy --preview
nextrs deploy
```

For apps with multiple bundles, `nextrs deploy` prepares the complete browser
client once, builds each server executable, copies public assets, and uploads
one prebuilt deployment. It preserves configured Vercel regions and cron paths.
Custom `[vercel]` build/install commands and raw `extra` settings are currently
rejected for split deployment rather than silently dropped. Advanced deployments
can explicitly adapt output from `nextrs bundles build --vercel`.

The platform routes directly to the owning function. All methods for a URL go to
that owner, allowing Axum to preserve HEAD and method-not-allowed behavior.
Static routes precede dynamic and catch-all routes. Unknown paths reach the
default function for 404 rendering. Internal function URLs are blocked from
public dispatch. Cookies, authorization, query strings, request bodies, response
headers, and streaming continue through the normal executable runtime adapter.

The generated Vercel project configuration deliberately rejects stock cloud
builds for split apps: use the prebuilt commands so a monolithic executable cannot
accidentally be deployed in place of the selected bundles.

## Shared behavior and current limits

Ancestor middleware, layouts, loading states, and not-found conventions required
by a selected route are included with it. Shared state is process-local: use an
external database or service when bundles must share data.

Prefetch-backed React pages must remain in `default` for this first version.
Moving one into a separate bundle produces a build error. Direct Rust calls
across a bundle boundary are not converted into RPC; cross-bundle prefetch and
remote server calls are future work. Full OpenAPI/client generation runs against
the unpartitioned application before building server bundles.

The [React Todos example](https://github.com/drewhirschi/nextrs/tree/main/examples/react-todos)
demonstrates `/api/exports` in an `exports` bundle with an optional CSV dependency
and a private resource directory. The default function omits both.
