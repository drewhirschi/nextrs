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

### App Builder / Scaffolder Command

Status: shipped as the `create-nextrs-app` workspace crate.

Nextrs has a first-class starter command, similar in spirit to `create-next-app`
or the old `create-react-app`. `create-nextrs-app` generates a React-first
starter, and the local dev workflow runs through `cargo-nextrs-dev` (installed
with `cargo install cargo-nextrs-dev`), which the scaffold wires up as a
`cargo dev` alias.

The scaffold is intentionally small but covers the important framework seams:

- A pure React route: `app/page.tsx`.
- A React route backed by Rust server code: `app/slow/` pairs a `page.tsx` with
  a `props.rs` that returns a `nextrs::QuerySeed` (seeding the React Query cache)
  and a `loading.tsx` streaming fallback.
- A Rust API route at `app/api/ping/route.rs` using `#[nextrs::api]`, plus a
  typed React Query client generated into `client/` by orval.
- The local workflow: `cargo dev` (alias for `nextrs-dev --bin <crate>`) for
  watch/restart.
- The Vercel bundling escape hatch: `NEXTRS_SKIP_BUNDLE=1` for deploy/codegen
  situations, `NEXTRS_SKIP_BUNDLE=0` (the default) for local dev.

Generated starter shape:

```text
my-app/
├── app/
│   ├── layout.tsx                  # React root layout
│   ├── page.tsx                    # pure client-rendered React page
│   ├── slow/
│   │   ├── page.tsx                # React page seeded from Rust props
│   │   ├── props.rs                # async props() -> nextrs::QuerySeed
│   │   └── loading.tsx             # streaming loading fallback
│   └── api/ping/
│       └── route.rs                # Rust GET handler with #[nextrs::api]
├── client/                         # orval-generated typed React Query client
├── src/main.rs                     # local Axum server
├── src/bin/dump-openapi.rs         # OpenAPI dump used for client codegen
├── build.rs                        # emit_registry + bundle_pages
└── .cargo/config.toml              # `dev` alias -> cargo-nextrs-dev
```

## Framework Surface

- `error.{rs,html}` segment convention.
- Per-route Vercel binaries for very large apps where the current single binary
  becomes too broad.
- Richer `route.rs` diagnostics and request extraction conventions.
- Nested streaming/Suspense-style boundaries beyond the current single loading
  slot per route.
- Upstream Vercel adapter support for streaming `text/html`, so
  `StreamingVercelLayer` can eventually become unnecessary.
