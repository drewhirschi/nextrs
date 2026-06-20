# Roadmap

This is a working roadmap, not a release commitment. Items here are things we
expect to revisit as real apps expose enough friction to justify the work.

## Dev Experience

### React HMR / Fast Refresh

Status: on the roadmap; no specific implementation plan or timeline yet.

Today `cargo dev` gives a watch-and-restart loop plus browser live reload. That
is enough for the current TSX support, but it is not React HMR: edits rebuild
the bundle, reload the page, and remount React from scratch.

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

Status: on the roadmap; document the desired shape before implementing.

Nextrs should eventually have a first-class starter command, similar in spirit
to `create-next-app` or the old `create-react-app`. The exact distribution is
still open; likely candidates are a `nextrs` CLI, a `cargo-nextrs` subcommand,
or a template generator invoked from `cargo`.

The initial scaffold should be intentionally small but cover the important
framework seams:

- A pure React route: `app/page.tsx`.
- A React route backed by Rust server code: a `page.tsx` that calls a typed
  client hook generated from a `route.rs` API handler.
- The standard local workflow: `cargo dev` for watch/restart/live reload and
  `cargo dev-once` for a single foreground run, matching the template in
  `docs/local-dev-workflow.md`.
- The Vercel bundling escape hatch documented clearly:
  `NEXTRS_SKIP_BUNDLE=1` for deploy/codegen situations, `NEXTRS_SKIP_BUNDLE=0`
  for local dev.

Example starter shape:

```text
my-app/
├── app/
│   ├── page.tsx                    # pure client-rendered React page
│   └── todos/
│       └── page.tsx                # calls the generated typed API hook
├── app/api/todos/
│   └── route.rs                    # Rust GET/POST handlers with #[nextrs::api]
├── client/                         # generated TypeScript/React Query client
├── src/main.rs                     # local Axum server
├── build.rs                        # emit_registry + bundle_pages
├── xtask/                          # local dev helper
└── .cargo/config.toml              # cargo dev / cargo dev-once aliases
```

Layouts are deliberately not locked into this first scaffold. We expect to
revisit layout ergonomics as TSX usage grows, including whether JavaScript-first
layouts should become more central than static HTML wrappers.

## Framework Surface

- `error.{rs,html}` segment convention.
- Per-route Vercel binaries for very large apps where the current single binary
  becomes too broad.
- Richer `route.rs` diagnostics and request extraction conventions.
- Nested streaming/Suspense-style boundaries beyond the current single loading
  slot per route.
- Upstream Vercel adapter support for streaming `text/html`, so
  `StreamingVercelLayer` can eventually become unnecessary.
