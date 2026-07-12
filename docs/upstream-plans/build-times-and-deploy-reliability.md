# Build Times and Deploy Reliability

- **Reported-in:** onenote-extractor
- **Date:** 2026-07-11
- **Status:** open

## Problem

nextrs apps accumulate slow, unreliable builds by default, and every app
rediscovers the same fixes. Concrete failures from onenote-extractor:

1. **Vercel git-integration builds can never succeed** for a nextrs app
   unless explicitly configured, but they run (and fail) on every push
   anyway. The default `@vercel/rust` builder compiles `api/*.rs`, which runs
   `build.rs` → `bundle_pages`, which needs `client/node_modules` and the
   generated client — neither exists in a bare git checkout (the scaffold now
   correctly gitignores `client/src/generated/`). Result: **~14 minutes of
   dependency compilation, then a guaranteed failure**, on every push, in
   every project that connects the GitHub integration. onenote-extractor
   burned six such builds in one afternoon before disabling
   `git.deploymentEnabled` in vercel.json.

2. **The failure surfaces after the expensive part.** `bundle_pages` checks
   for `client/node_modules` only when it runs — after cargo has compiled the
   full dependency tree. A missing-prereqs check that fails in the first
   seconds would turn a 14-minute failure into a 10-second one.

3. **Build times balloon quietly.** A release build of a modest nextrs app is
   ~15-25 min cold on 2-core CI runners. Contributors: the rolldown/oxc tree
   compiled as a build-dependency of every consumer crate, `cargo build` +
   `cargo test` each linking all targets (CI often runs both; `cargo test`
   alone builds everything), per-job CI caches that never warm each other,
   and (app-specific but common) vendored native deps like OpenSSL.

## Observed numbers (onenote-extractor, GitHub Actions, 2-core)

| configuration | wall time |
|---|---|
| cold, per-job caches, build + test as separate steps | 25-29 min |
| warm cache, redundant build step folded into `cargo test` | ~5-7 min |
| Vercel git build (always fails) | 14-19 min per push |

## Proposed Direction

1. **Scaffold vercel.json should decide the deploy story explicitly.** Either
   emit the full git-build config (installCommand/buildCommand, as
   react-todos now has) or emit `"git": {"deploymentEnabled": false}` with a
   comment pointing at the prebuilt flow. The current silent default — git
   builds that fail after 14 minutes — is the worst of both.

2. **Fail fast in `bundle_pages`.** Check for `client/node_modules` and the
   generated client BEFORE cargo compiles anything — e.g. a preflight in
   `build.rs` emitted by the scaffold, or documented `rerun-if` ordering so
   the bundler's prereq check runs first. Print the exact fix command
   (`cd client && npm ci && npm run gen`).

3. **Ship a reference CI workflow in the scaffold** (`.github/workflows/ci.yml`)
   encoding the hard-won details:
   - `npm run gen` before any cargo command (NEXTRS_SKIP_BUNDLE chicken-egg);
   - one `cargo test --release` step, no separate `cargo build` (test builds
     every target already — a separate build step is pure link-time waste);
   - `Swatinem/rust-cache` with a `shared-key` so all jobs warm one cache,
     `cache-on-failure: true` so cold failing runs still populate it;
   - `concurrency` + `cancel-in-progress` so pushes don't queue stale builds;
   - node pinned to match the version that writes package-lock.json (npm 10
     rejects lockfiles npm 11 writes when nested platform-specific optional
     deps lose their `optional: true` flag).

4. **Document a build-time budget.** "A warm CI run of a scaffolded app
   should stay under ~6 minutes" is a testable claim the example app's CI can
   enforce; regressions in the rolldown build-dep tree would show up there
   instead of in every downstream app.

## Validation

- Scaffold an app, connect the GitHub integration with no extra config: no
  doomed builds run (or the configured git build succeeds).
- Delete `client/node_modules`, run `cargo build`: failure within seconds,
  message names the fix.
- Example-app CI: warm run wall time asserted under budget.
