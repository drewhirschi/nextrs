# nextrs — Issues backlog

Captured from a 4-area audit (cargo dev, create-nextrs-app, doc↔impl drift, Vercel build) on 2026-06-27.
Status key: ✅ fixed this pass · 🔧 fixing · 🔒 needs a decision or a release (not auto-fixable) · ☐ open.

> ⚠️ **Cross-cutting — docs vs. unmerged work.** Some of the new positioning references features that are NOT yet on `main`:
> - **prefetch rename** (seeds→prefetch) — unmerged (`refactor/prefetch-and-speculation-rename`, not yet run). Main still ships `props.rs` / `QuerySeed` / `seed_key` / `__nx_seeds__`.
> - **TanStack Router soft-nav** ("instant navigation") — on unmerged `fix/dashboard-rs-aliases`. Main's instant-nav today is **browser Speculation Rules** (the `prefetch`/speculation module), not a client router.
> - **not-found convention** — PR #22 (`feat/not-found-convention`), unmerged.
>
> Doc fixes below describe **what is actually on main** (React `.tsx` pages, `props.rs`/seeds, rolldown bundler, Speculation-Rules nav-prefetch). The aspirational positioning (TanStack Router, "prefetch") is kept in the top-of-README/MANIFEST manifesto but should be reconciled when those branches land. See **DOC-12**.

---

## Vercel build (project `nextrs-docs`, builds from root `.`)
Root cause: the root deploy config is missing 3 things the working `examples/react-todos` already has. Not caused by the React/JSX bundler work (docs site has no `.tsx`).

- ✅ **VRC-1 (blocker)** — `vercel.json` had no `functions` runtime; `vercel-rust` is a community runtime and isn't auto-detected, so `api/index.rs` never builds → every route 404s. **Fix:** added `functions: { "api/index.rs": { "runtime": "vercel-rust@4.0.11" } }`.
- ✅ **VRC-2 (blocker)** — root `.cargo/config.toml` had no `[build]` table; `vercel-rust` dereferences `config.build.target` and crashes (`Cannot read properties of undefined (reading 'target')`). Regression from adding the `dev` aliases. **Fix:** added an empty `[build]` table (kept the aliases).
- ✅ **VRC-3 (blocker)** — no root `rust-toolchain.toml`; workspace is edition 2024 (needs rustc ≥ 1.85) but Vercel's image ships an older default. **Fix:** added `rust-toolchain.toml` pinning `1.96.0`.
- 🔧 **VRC-4 (high)** — `site/content/docs/deploy-vercel.md` is stale ("Vercel auto-detects Cargo.toml", `vercel.json` with only a rewrite) and is the source of the broken config. **Fix:** rewrite to require the runtime block, toolchain pin, and `[build]` table.
- ☐ **VRC-5 (medium)** — root `public/` (CDN static assets, incl. `/style.css`) is gitignored and generated at build time; a Git-connected deploy starts with no `public/`, so assets may 404. **Fix options:** commit built `public/`, or serve from `site/public/`, or emit into Vercel's output dir. Verify `/style.css` → `x-vercel-cache: HIT` post-deploy.
- ✅ **VRC-6 (info)** — ruled out: recent JSX/bundler commits don't affect the docs build (no `.tsx`, `build` feature only, no node step).

## `cargo dev` first-run friction
- 🔧 **DEV-1 (high)** — site hardcodes `:3000` and `.unwrap()`s the bind → raw `AddrInUse` panic with no hint (reproduced: `music-agg` holds :3000). **Fix:** match the bind error, print an actionable message + auto-increment to a free port and log it; same in the `create-nextrs-app` template. Document the `PORT` env var in README.
- 🔒 **DEV-2 (high)** — two divergent dev runners: this repo's `cargo dev` → `xtask`, but scaffolded apps → `cargo-nextrs-dev` (`nextrs-dev --bin`). Alias/README/`local-dev-workflow.md` all call xtask "canonical." **Decision needed:** pick one (migrate repo to `cargo-nextrs-dev`, or document xtask as repo-internal legacy). Doc side fixed in DEV-3/DOC-7.
- 🔧 **DEV-3 (medium)** — `docs/local-dev-workflow.md` presents xtask as canonical and says "copy xtask/". **Fix:** rewrite around `cargo-nextrs-dev`.
- 🔧 **DEV-4 (medium)** — README implies `cargo dev` bundles React though the demo site is pure Rust; no "Node/npm not required" note. **Fix:** add a prerequisites line (Rust only; Node optional, for `site/client` typed client).
- 🔧 **DEV-5 (medium)** — scaffolded apps fail `cargo dev` with cryptic `no such command: nextrs-dev` if the install step is skipped. **Fix:** scaffolder prints a clear prerequisite + maps the error to the install command.
- 🔧 **DEV-6 (low)** — no `.env.example`; `PORT` override undocumented. **Fix:** add `.env.example` + README note.

## `create-nextrs-app`
- 🔒 **CRA-1 (blocker)** — default flow pins `nextrs = "0.3"` but crates.io max is `0.2.2`; the generated code targets unreleased 0.3 APIs, so even `0.2` won't compile. **Needs release:** publish `nextrs` 0.3.0 (+ macros); keep the scaffold's VERSION in lockstep (verify in CI).
- 🔒 **CRA-2 (blocker)** — `cargo dev` impossible for a crates.io user: `cargo-nextrs-dev` is unpublished (`publish = false`), so `cargo install cargo-nextrs-dev` fails. **Needs release/decision:** publish it, or ship the watcher as a subcommand of the published crate, or print a `cargo run` fallback.
- 🔒 **CRA-3 (high)** — the scaffolder itself is unpublished and undocumented (no `cargo install create-nextrs-app`, not in README/docs). **Needs release:** publish it; add a README quickstart.
- 🔧 **CRA-4 (medium)** — generated workflow is fully manual with a build-order trap (npm install → npm run gen → cargo build). **Fix:** scaffolder optionally runs the steps and/or writes a project README with the exact ordered commands + `.env.example`. (Local part fixable now.)
- 🔧 **CRA-5 (low)** — scaffold's `props.rs` hand-builds a seed entry vs. the flagship example's typed companion. **Fix:** align or add a pointer comment. (No correctness issue.)
- ✅ **CRA-6 (info)** — verified: with `--nextrs-path` the full create → npm → gen → build → `cargo dev` flow works and matches current conventions.

## Documentation ↔ implementation drift
- 🔧 **DOC-1 (blocker)** — `MANIFEST.md` Non-goals still says "No client-side framework. No htmx, no React, no JS bundle" — false and self-contradicting. **Fix:** rewrite Non-goals; add `.tsx`/`props.rs` to Conventions.
- 🔧 **DOC-2 (high)** — published Getting Started says "No client-side framework", never mentions React/`.tsx`/`props.rs`, cites `0.1`. **Fix:** add a React track; remove the claim; bump version.
- 🔧 **DOC-3 (high)** — Routing Conventions omits `page.tsx`/`layout.tsx`/`loading.tsx` and `props.rs`. **Fix:** add `.tsx` rows + a `props.rs` row + React-pages subsection.
- 🔧 **DOC-4 (high)** — README body (Quick look, conventions table, Status, Project layout) is all the old HTML design; lists scaffolder as future work, "51 tests" (actual 121), omits `bundle/prefetch/seed/openapi/docs` modules. **Fix:** update tree/table/status/layout/test count.
- 🔧 **DOC-5 (high)** — `react-server-props.md` is filed under Roadmap/"(Preview)", future tense, and says the bundler is `swc` (it's rolldown). **Fix:** move to Guides, present tense, swc→rolldown.
- 🔧 **DOC-6 (medium)** — ROADMAP + README list the app scaffolder as unbuilt. **Fix:** mark shipped (`create-nextrs-app`), fix the xtask-vs-`cargo-nextrs-dev` sample.
- 🔧 **DOC-7 (medium)** — `local-dev-workflow.md` xtask-centric (see DEV-3).
- 🔧 **DOC-8 (medium)** — Deploy-to-Vercel guide omits React deploy reqs (toolchain pin, `NEXTRS_SKIP_BUNDLE=1`, committed `public/dist`) — and now also the VRC-1..3 fixes. **Fix:** add a "Deploying a React app" + corrected config section.
- 🔧 **DOC-9 (medium)** — `MANIFEST.md` body (Layout/Conventions/Where-to-look/Tests) describes pre-React module/route shape. **Fix:** add modules + `.tsx`/`props.rs` + refresh test count.
- 🔧 **DOC-10 (low)** — streaming docs frame streaming as the central UX + deny a client framework. **Fix:** reframe as one of two rendering models.
- 🔧 **DOC-11 (low)** — version pins inconsistent across docs (0.1/0.2 vs crate 0.3.0). **Fix:** normalize to 0.3.
- 🔒 **DOC-12 (decision)** — positioning claims (TanStack Router instant-nav, "prefetch" naming) outrun `main` (see cross-cutting note). **Decision:** land the unmerged branches, or scope the manifesto to shipped state.
