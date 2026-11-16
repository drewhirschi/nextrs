# Roadmap

This is a working roadmap, not a release commitment. Items here are things we
expect to revisit as real apps expose enough friction to justify the work.

## Dev Experience

### React HMR / Fast Refresh

Status: on the roadmap; no specific implementation plan or timeline yet.

Today `cargo dev` is intended to provide watch/restart plus full-page browser
reload through `tower-livereload` in debug builds. That is the baseline dev
experience, but it is not React HMR: edits rebuild the bundle, reload the page,
and remount React from scratch.

Real React HMR should preserve compatible component state by updating changed
modules in place. This should be feasible to explore without abandoning the
Rust-first architecture because the relevant JavaScript toolchain pieces have
strong Rust implementations or Rust ties:

- Rolldown for bundling and module graph work.
- OXC for TypeScript/JSX transforms and React Refresh support.
- SWC as another mature Rust-based transform path.

The likely shape is a dev-only asset path that handles transforms, dependency
graph invalidation, websocket updates, React Refresh runtime wiring, and full
reload fallback. Production should remain static bundles served by the Rust
app. We will revisit this when live reload becomes painful enough in TSX-heavy
development.

### Unified CLI and App Scaffolder

Status: shipped in the `cargo-nextrs` workspace crate; first crates.io release
of the unified package is pending.

Nextrs has a first-class starter command, similar in spirit to `create-next-app`
or the old `create-react-app`. One `cargo install cargo-nextrs` provides both
`nextrs` and `cargo nextrs`; either can create, run, and regenerate an app:

```bash
nextrs new my-app                 # or: cargo nextrs new my-app
nextrs dev                        # or: cargo nextrs dev
nextrs client generate            # or: cargo nextrs client generate
```

The scaffold retains `cargo dev` as a project-local alias. The old
`create-nextrs-app` and `cargo nextrs-dev` executables are deprecated
compatibility wrappers.

The scaffold is intentionally small but covers the important framework seams:

- A pure React route: `app/page.tsx`.
- A React route backed by Rust server code: `app/slow/` pairs a `page.tsx` with
  a `prefetch.rs` that returns a `nextrs::QuerySeed` (seeding the React Query cache)
  and a `loading.tsx` streaming fallback.
- A Rust API route at `app/api/ping/route.rs` using `#[nextrs::api]`, plus a
  framework-independent Fetch client and React Query integration generated
  into the hidden `.nextrs/client` npm workspace.
- Shared React UI under `components/`, with arbitrary non-route modules also
  allowed beside convention files in `app/`.
- The local workflow: `cargo dev` (alias for `nextrs dev --bin <crate>`) for
  watch/restart, while direct dev commands infer `default-run` when possible.
- Automatic root `npm install` and client generation for fresh apps, with
  `--no-install` for a files-only scaffold.
- The Vercel bundling escape hatch: `NEXTRS_SKIP_BUNDLE=1` for deploy/codegen
  situations, `NEXTRS_SKIP_BUNDLE=0` (the default) for local dev.

Generated starter shape:

```text
my-app/
├── app/
│   ├── layout.tsx                  # React root layout
│   ├── page.tsx                    # React page
│   ├── PingDemo.tsx                # freely colocated non-route component
│   ├── slow/
│   │   ├── page.tsx                # React page seeded from Rust props
│   │   ├── prefetch.rs             # async prefetch() -> nextrs::QuerySeed
│   │   └── loading.tsx             # streaming loading fallback
│   └── api/ping/
│       └── route.rs                # Rust GET handler with #[nextrs::api]
├── components/
│   └── NextrsLogo.tsx              # shared React component
├── .nextrs/
│   ├── client/                     # generated package; do not edit
│   └── dump-openapi.rs             # hidden OpenAPI helper
├── src/
│   ├── app.rs                      # shared Rust application/router
│   └── main.rs                     # local/container entry
├── api/index.rs                    # Vercel adapter
├── build.rs                        # emit_registry + bundle_pages
└── .cargo/config.toml              # `dev` alias -> unified nextrs CLI
```

## Framework Surface

### RSX server components with React islands

Status: v1 implemented 2026-09-12 (same PR as the design) — `rsx!` macro,
oxc-based props extraction, island bundling with typed `crate::client`
bindings, and the `pub async fn page(...)` convention; demo at
`examples/react-todos/app/server-stats/`. Remaining: scaffold + docs-site
adoption, builder-style bindings for optional props, cross-file type imports.
Full design in [docs/rsx-server-components.md](docs/rsx-server-components.md).

Replace Askama templating for Rust-rendered pages with a JSX-shaped `rsx!`
macro: `page.rs` becomes a server component (DB access, auth, then HTML), and
imported React `.tsx` components render as typed, hydrated islands. Build-time
codegen (on the already-vendored OXC crates) parses each island's TypeScript
props interface and generates Rust bindings, so island usage type-checks
across the language boundary. No `'use client'` directive (the `.rs`/`.tsx`
split is the boundary) and no `#[page]` macro (`route.rs`-style convention).
First step is a props-extractor spike; `site/` is the dogfood target for
migrating off Askama.

### Typed API error contracts

Explore making `Result` the standard return shape for generated-client routes:

```rust
#[nextrs::api]
pub async fn get() -> Result<Json<Greeting>, ApiError> {
    // ...
}
```

Define a nextrs error-response trait that lets `ApiError` expose every possible
HTTP status and response body at build time. The API macro could then infer
both success and error variants for OpenAPI and generated clients without
repeating `responses(...)` metadata on each handler.

Questions to resolve before enforcing this:

- Whether all `#[nextrs::api]` handlers must return `Result`, or whether plain
  `Json<T>` remains valid for genuinely infallible routes.
- How enum variants map to statuses, schemas, and descriptions.
- How to support dynamic `IntoResponse` implementations without claiming an
  incomplete contract.

### OpenAPI-first output, opt-in client generators

Change the default contract: nextrs should just generate the OpenAPI spec
document. Automatic client generation becomes a feature you enable, not
something every app gets.

- **Default:** emit the OpenAPI spec only. No `.nextrs/client` package, no npm
  codegen step unless asked for.
- **Opt-in generators**, each enabled explicitly in `nextrs.toml`:
  - TypeScript fetch client — the base layer.
  - React Query client — sits on top of the TypeScript client rather than
    being its own generator; enabling it implies the TypeScript client.
  - Swift client.
  - Rust client.
  - Room for others, since everything is driven from the spec.

Questions to resolve:

- The shape of the `nextrs.toml` section, and what `nextrs client generate`
  does when nothing is enabled.
- Whether non-TypeScript generators are first-party or delegate to existing
  OpenAPI generators.
- Migration for existing apps and the scaffold, which assume the TypeScript +
  React Query client is always generated; `examples/react-todos` and RSX
  islands' typed `crate::client` bindings depend on it today.

- Per-route Vercel binaries for very large apps where the current single binary
  becomes too broad.
- Make the idiomatic Rust `src/main.rs` usable as the Vercel function entry too.
  Today Vercel's builder rejects `functions` patterns outside `api/` for custom
  runtimes, so nextrs keeps a tiny `api/index.rs` adapter. Desired end state:
  Vercel allows an explicit `functions["src/main.rs"]` entry, letting generated
  apps avoid the extra deploy-only file.
- Richer `route.rs` diagnostics and request extraction conventions.
- Add a `nextrs::server::bind_listener` helper: honor an explicit `PORT`
  strictly, otherwise try local ports 3000 through 3009. Generated entry
  points should call the shared helper instead of carrying duplicated binding
  loops; until then use `PORT=<port> cargo dev` when 3000 is occupied.
- Nested streaming/Suspense-style boundaries beyond the current single loading
  slot per route.
- Upstream Vercel adapter support for streaming `text/html`, so
  `StreamingVercelLayer` can eventually become unnecessary.

## Deploy & Runtime

### AWS Lambda deploy target

Status: proposed 2026-09-23; not started.

Vercel's free-tier logs are too thin to operate real apps on (short retention,
no querying, and log drains require Pro). The goal is a second first-class
target: native Rust on AWS Lambda, with CloudWatch as the baseline for logs
and observability. Coolify and Dokploy don't fit here. They run long-lived
containers on a VPS. Cloudflare Workers run Rust only as WASM, and the free
plan caps CPU at 10ms per request.

Shape:

- **Adapter:** `lambda_http::run(app)` takes the Axum `Router` from
  `src/app.rs` directly, so this is one more process adapter beside
  `api/index.rs`, not a new architecture. Build with `cargo lambda build
  --arm64` (`provided.al2023`).
- **Ingress:** a Lambda Function URL (streaming responses, no API Gateway
  cost), fronted by CloudFront for the custom domain and caching. Bundled
  static assets go to S3 behind the same distribution.
- **Logs:** set `tracing-subscriber` to JSON output and enable Lambda's
  native JSON log format and levels, with route and request ID on every span,
  so CloudWatch Logs Insights can answer "p99 by route" and "errors in the
  last hour". Set explicit retention (7–14 days). An optional subscription
  filter or OTel layer can forward to Axiom or Grafana Cloud.
- **Deploy:** `nextrs deploy --target aws` generates the infrastructure (CDK
  or Terraform, TBD) from `nextrs.toml`, the same way `vercel.json` is
  generated today. Crons become EventBridge Scheduler rules.
- **Dogfood:** hhh first, since cold-start telemetry already exists to
  compare against.

Semantics that differ from Vercel, which must be designed rather than
papered over:

- **`WaitUntil`:** Lambda freezes the instance once the response returns.
  Options: keep a streamed response open until the tasks finish, use an
  internal Lambda extension, or hand the work to SQS or to durable execution
  (below).
- **Concurrency:** Lambda runs one request per instance, unlike Fluid's
  in-instance concurrency. Expect more (cheap) cold starts. Re-run the
  arrival-shape analysis from `docs/coldstart-arrival-shapes.md`.
- **Previews:** there is no built-in per-branch preview. Use a stage or alias
  per branch, or adopt SST if that becomes the main pain.

Questions to resolve:

- CDK vs Terraform vs SST for generated infrastructure, and whether nextrs
  owns the stack or emits it for the app to own.
- Whether the Vercel and Lambda adapters can share one entry point with a
  cfg/feature switch.
- How `/__nx/health` and the pinger workflow map onto Lambda.

### Durable execution on object storage (S3 / R2)

Status: proposed 2026-09-23; not started. Supersedes the "background jobs
behind WaitUntil" idea. `WaitUntil` becomes one way to *drive* a run, not
the durability layer.

S3 added conditional writes in 2024: `If-None-Match: *` (create only if
absent) and `If-Match: <etag>` (compare-and-swap). Together with S3's
strong read-after-write and LIST consistency, this makes a bucket enough to
build a correct durable-execution journal on. No database, queue service, or
Temporal cluster is needed. **R2 supports the same primitives**: its S3 API
implements `If-Match`, `If-None-Match`, `If-Modified-Since`, and
`If-Unmodified-Since` on PutObject, plus the copy-source equivalents on
CopyObject. So the same design runs on AWS, on Cloudflare, or next to a
Vercel app with a free-tier R2 bucket.

Developer surface (sketch):

```rust
#[nextrs::workflow]
pub async fn onboard(ctx: Ctx, user: UserId) -> Result<()> {
    let acct = ctx.step("create-account", || create_account(user)).await?;
    ctx.sleep("wait-a-day", Duration::from_secs(86_400)).await?;
    ctx.step("send-welcome", || send_email(acct.email)).await?;
    Ok(())
}

// Typed, idempotent start. Returns a RunId.
onboard::start(user).await?;
```

Storage layout and protocol:

- `runs/{id}/input.json` is written once (`If-None-Match: *`). The run ID
  doubles as the idempotency key.
- `runs/{id}/steps/{n}-{name}.json` is written once per completed step. On
  replay, completed steps return their recorded output instead of
  re-executing. Write-once makes a duplicate executor harmless.
- `runs/{id}/state.json` holds status, cursor, lease owner and expiry, and
  wake-at time. Every transition is a CAS on its ETag (`If-Match`). Taking
  over an expired lease is a CAS too, so two executors can't both own a run.
- Wake-ups go in an index such as `due/{wake_at}-{id}` that a sweeper LISTs.
  Deletion must not depend on conditional deletes, since R2 doesn't list
  conditional DeleteObject. Stale index entries are cleaned up only after the
  CAS on `state.json` confirms them.

Execution drivers (pluggable; the journal is the same everywhere):

- Inline after the response via `WaitUntil` (Vercel), bounded by
  `maxDuration`. A step that exceeds its budget yields and resumes on the
  next wake.
- A cron sweeper (`nextrs.toml` crons / EventBridge / the Cloudflare cron
  shim) that picks up due and orphaned runs.
- Optionally SQS or Cloudflare Queues for low-latency wake-ups.

Implementation notes:

- Build on the `object_store` crate: `PutMode::Create` and
  `PutMode::Update(etag)` map onto the conditional headers, and its
  `LocalFileSystem` backend gives `nextrs dev` a zero-setup store.
- Prior art: turbopuffer's queue-in-a-JSON-file-on-object-storage, SlateDB,
  and Vercel Workflow / Inngest / Restate for the programming model.
- Cost is dominated by Class A ops (writes plus LIST). Measure per-step op
  counts and batch where possible. R2's free tier (1M Class A/month) should
  cover small apps.

Questions to resolve:

- Determinism rules for workflow bodies (everything non-deterministic goes
  through `ctx.step`), and how to catch violations. Replay can be checked by
  step name and sequence.
- Versioning in-flight runs across deploys.
- Visibility: a `/__nx/runs` dashboard or CLI (`nextrs runs ls/show/retry`)
  that reads the bucket directly.
- Retry, back-off, and dead-letter policy per step.
- Whether `ctx.sleep` below the sweeper interval needs the queue driver.

### Build artifact cache (push builds, not just code)

Status: proposed 2026-09-24; not started. Design in
[docs/build-artifact-cache.md](docs/build-artifact-cache.md).

There are two layers. **sccache on R2** shares compiled crates between
laptops and CI (off the shelf, so start there). An **output cache** stores the
verified `.vercel/output` (or a Lambda zip) under a key built from the git
tree hash, toolchain, target, and build config. `nextrs deploy` and CI pull on
a hit and skip cargo entirely. Uploads are write-once (`If-None-Match: *`),
using the same object-storage primitive as durable execution. The open
decision is trust: a cache hit ships a binary CI didn't compile, so write
access to production-consumed keys starts out limited to `main` CI.
