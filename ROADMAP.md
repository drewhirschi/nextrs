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
