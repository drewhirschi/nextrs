# Building nextrs apps on Vercel — what costs what

A running record of deploy-time measurements and the optimizations applied,
so future changes can be judged against numbers instead of vibes. Measured on
the docs site (`nextrs-docs`, Vercel 2-core/8GB builder) unless noted.

## Anatomy of a deploy

`vercel.json` drives it: `installCommand` (npm ci in `client/`), `buildCommand`
(client codegen, then `cargo build --release`), then the `vercel-rust` runtime
packages `api/index.rs` from the same target dir. The build cache Vercel
restores covers npm — **not** the cargo target dir, so every deploy is a cold
Rust build. That makes the Rust compile profile and the number of cargo
invocations the entire game; the JS steps are noise.

## Baseline — 2026-07-12, commit c908a5d (10m 0s total)

| Phase | Time | Share |
|---|---|---|
| clone + cache restore | 1s | — |
| `npm ci` | 9s | 1.5% |
| rustup toolchain sync (1.96.0 pin) | 13s | 2% |
| `cargo run dump-openapi` — full **debug** build of ~700 crates to run a 0.2s binary | 3m 42s | 37% |
| orval client gen | 1s | — |
| `cargo build --release -p site` under `lto="fat"`, `codegen-units=1` | 5m 04s | 51% |
| bundle mirror + function packaging + upload | ~45s | 7.5% |

Two full cold compiles of the workspace per deploy, the second one under
max-optimization settings on 2 cores.

## Optimizations applied (2026-07-12)

1. **Release profile back to cargo defaults.** The workspace `[profile.release]`
   had `lto = "fat"` + `codegen-units = 1` since the initial commit — settings
   that serialize codegen and add a huge single-threaded LTO link, for zero
   observable benefit to a docs site. Max-opt now lives in an explicit
   `[profile.perf]` for benchmark runs. (`benchmarks/results/results.md`
   numbers predating this change were measured under the old fat-LTO release.)
2. **No Rust compile for client codegen on Vercel.** `buildCommand` runs orval
   directly against the committed `client/openapi.json` instead of
   `npm run gen` (which cargo-builds the whole workspace in debug just to run
   `dump-openapi`). `npm run gen` stays the local workflow; CI fails if the
   committed spec drifts from the code ("Committed OpenAPI specs are fresh"
   step in `.github/workflows/ci.yml`). Applied to both `site/` and
   `examples/react-todos/`.

## Results

| Date | Commit | Build | Notes |
|---|---|---|---|
| 2026-07-12 | c908a5d | **10m 0s** | baseline above |
| 2026-07-12 | 8cd4509 | **6m 0s** | −40%. Fix 2 delivered exactly as predicted (codegen: 3m43s → 1s). Fix 1 delivered ~nothing: release build 5m04s → **5m06s** — see finding below. |

**Finding: fat LTO was not the bottleneck on a 2-core builder.** Dropping
`lto="fat"`/`codegen-units=1` left the release build time unchanged (5m04s →
5m06s). Compiling the ~700-crate dependency graph dominates at this core
count; the serial LTO link and single-CGU codegen it replaced were noise.
The profile change stays (it still speeds up many-core local release builds
and a docs site gains nothing from max-opt), but the projected "release step
≈ half" was wrong. Remaining big levers, in order: cache the cargo target dir
(kills the cold build entirely when it fits), more builder cores, fewer
dependencies. And queue time isn't build time: this measurement sat ~100
minutes in Vercel's queue behind another project's deploys — one build slot
per account is its own bottleneck (see nextrs-apps-build-audit.md).

## Not done (deliberately), and why

- **Caching the cargo target dir** (`CARGO_TARGET_DIR` under `.vercel/cache`):
  could take warm deploys to ~1–2m, but Rust target dirs routinely exceed
  Vercel's build-cache size limit, at which point the cache silently stops
  sticking. Try only if the cold-build time above still chafes.
- **Bigger build machine**: project-settings toggle, pure cores-for-money;
  stacks with everything here. No repo change involved.
- **Trimming release-built dev conveniences** (`tower-livereload`, `dotenvy`):
  seconds, not minutes. Not worth the churn yet.
