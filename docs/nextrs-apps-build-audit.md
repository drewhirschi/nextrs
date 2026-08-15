# nextrs apps in ~/work — build-setup audit (2026-07-12)

Prepared for review; **no changes made to any app**. Context: the framework
repo's docs-site deploy was just taken from 10m → ~3m (see
`nextrs/docs/building-on-vercel.md`) by (1) dropping fat-LTO from the release
profile and (2) not cargo-building the workspace in debug just to regenerate
the typed client on Vercel. This checks whether the other apps have the same
problems, plus general build health.

## TL;DR table

| App | nextrs version | Fat LTO? | Double cargo build on deploy? | Convention era | Verdict |
|---|---|---|---|---|---|
| hhh-next | **=0.2.1 (4 minors behind)** | no | no (committed dist + skip-bundle) | `props.rs` (era-correct) | works, but old framework |
| onenote-extractor | vendored **0.3.3** | no | no | `prefetch.rs` ✓ | healthiest; vendor is stale |
| music-stats-aggregator | **absolute local path dep** | no | n/a (no deploy config) | `prefetch.rs` ✓ | not deployable as-is |

None of them have the fat-LTO problem — that was unique to the framework
workspace. None double-build for client codegen the way the docs site did.

## hhh-next

- **Framework: `nextrs = "=0.2.1"`**, the biggest gap in the fleet. Missing
  everything since 0.3.x: the prefetch.rs rename (its `props.rs` files are
  era-correct and keep working via back-compat), route params for tsx pages,
  app-shell soft navigation, the not-found convention, and — most relevant to
  build safety — **0.3.6's bundler guard** that fails the build on unresolved
  bare imports instead of shipping a dead page (the docs site shipped exactly
  that outage).
- Deploy: bun installs, then `cargo release-nextrs` (an xtask alias) pre-builds
  `public/dist` with bundling on, while `.cargo/config.toml` sets
  `NEXTRS_SKIP_BUNDLE=1` so the later vercel-rust function build skips
  bundling. Clever and sound — no wasted compile. One release build per deploy.
- If/when it upgrades to 0.3.x: `cargo update` + rebuild is the framework
  side; scaffolded code (its `main.rs`, xtask) won't self-update — expect a
  small migration. The `props.rs` files can stay (legacy support) or be
  renamed file-by-file.

## onenote-extractor

- **Framework: vendored `vendor/nextrs` at 0.3.3** — three patches behind.
  Notably missing 0.3.6's unresolved-bare-import guard, so it can still ship
  the "green build, dead page" failure the docs site hit. A vendor refresh is
  a directory copy + rebuild.
- Deploy: no install/buildCommand — the vercel-rust function build does all
  compilation (one release build; bundling runs inside it via build.rs).
  Nothing wasteful. Committed `client/openapi.json` present.
- Uses `prefetch.rs` already. Pinned toolchain 1.96.0 matches the fleet.

## music-stats-aggregator

- **`nextrs = { path = "/home/drew/work/nextrs/crates/nextrs" }`** — an
  absolute path onto this machine. Builds only here; any CI/Vercel/other-host
  build fails at dependency resolution. If it's meant to deploy anywhere,
  switch to the published crate (`nextrs = "0.3"`), or vendor like
  onenote-extractor.
- No `vercel.json`, no Dockerfile — no deploy story at all yet. Nothing to
  optimize until it has one; when it does, copy the onenote-extractor shape
  (single function build, committed openapi.json, toolchain pin) which is the
  cleanest of the fleet.
- Conventions are current (`prefetch.rs`, 0.3.x scaffold).

## Fleet-wide observations

- All three pin `rust-toolchain.toml` to 1.96.0 — consistent, good.
- All three commit `client/openapi.json` — they could all adopt the framework
  repo's CI freshness check (`npm run gen && git diff --exit-code`) if they
  grow CI.
- The recurring theme isn't build waste — it's **version drift**: 0.2.1,
  0.3.3, and a local path. Anything fixed in the framework (like the bundler
  guard) protects an app only after it upgrades. A periodic "bump the fleet"
  pass would close that structurally.
