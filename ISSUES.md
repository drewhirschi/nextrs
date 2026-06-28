# nextrs — Issues backlog

Captured from a 4-area audit (cargo dev, create-nextrs-app, doc↔impl drift, Vercel build) on 2026-06-27.
Status key: ✅ fixed · 🔧 partial · 🔒 needs a decision or a release (not auto-fixable) · ☐ open.

**Progress (2026-06-27, branch `fix/audit-issues`):** Vercel build blockers fixed (VRC-1/2/3), `cargo dev` port panic fixed in site + scaffold (DEV-1), scaffold UX hint (DEV-5), `.env.example` (DEV-6), and the full doc-drift sweep (DOC-1…11, DEV-3, VRC-4). Remaining are release actions (publish crates) and product decisions.

> ⚠️ **Cross-cutting — docs vs. unmerged work.** Some positioning references features not yet on `main`:
> - **prefetch rename** (seeds→prefetch) — unmerged. Main ships `props.rs` / `QuerySeed` / `seed_key` / `__nx_seeds__`.
> - **TanStack Router soft-nav** ("instant navigation") — on unmerged `fix/dashboard-rs-aliases`. Main's instant-nav today is **browser Speculation Rules**, not a client router.
> - **not-found convention** — PR #22, unmerged.
>
> Doc fixes below describe **what's on main**. The aspirational positioning (TanStack Router, "prefetch") stays in the top-of-README/MANIFEST manifesto; reconcile when those branches land — see **DOC-12**.

---

## Vercel build (project `nextrs-docs`, builds from root `.`)
- ✅ **VRC-1 (blocker)** — `vercel.json` missing `functions` runtime → added `vercel-rust@4.0.11` runtime decl.
- ✅ **VRC-2 (blocker)** — root `.cargo/config.toml` had no `[build]` table (vercel-rust crashes) → added empty `[build]`.
- ✅ **VRC-3 (blocker)** — no root `rust-toolchain.toml` (edition 2024 needs ≥1.85; Vercel default older) → pinned `1.95.0` (matches local; docs site is pure Rust, no oxc).
- ✅ **VRC-4 (high)** — `deploy-vercel.md` rewritten to require the runtime block, toolchain pin, `[build]` table + a "Deploying a React app" section.
- ☐ **VRC-5 (medium)** — root `public/` is gitignored + build-generated; a Git-connected deploy may 404 static assets (`/style.css`). **Options:** commit built `public/`, serve from `site/public/`, or emit into Vercel output. Verify `x-vercel-cache: HIT` post-deploy.
- ✅ **VRC-6 (info)** — ruled out: JSX/bundler commits don't affect the docs build (no `.tsx`).
- ⏳ **Verification:** `vercel build` (local, no deploy) pending — config matches the proven `examples/react-todos` setup.

## `cargo dev`
- ✅ **DEV-1 (high)** — site (`site/src/main.rs`) + scaffold template now bind with a clean fallback (auto-increment to next free port, log it, exit with a message instead of a raw `AddrInUse` panic).
- 🔒 **DEV-2 (high)** — two dev runners (repo uses `xtask`; scaffolds use `cargo-nextrs-dev`). **Decision:** consolidate or keep xtask as repo-internal legacy. (Docs now describe both correctly — DEV-3.)
- ✅ **DEV-3 (medium)** — `local-dev-workflow.md` rewritten around `cargo-nextrs-dev`.
- ✅ **DEV-4 (medium)** — README "Run locally" now states the demo site is pure Rust (Node optional, only for `site/client`).
- ✅ **DEV-5 (medium)** — scaffolder prints ordered steps + a tip mapping `no such command: nextrs-dev` to the install.
- ✅ **DEV-6 (low)** — added `.env.example` documenting `PORT`.

## `create-nextrs-app`
- 🔒 **CRA-1 (blocker)** — scaffold pins `nextrs = "0.3"` but crates.io max is `0.2.2`; generated code needs 0.3 APIs. **Needs release:** publish `nextrs` 0.3.0 (+ macros); keep VERSION in lockstep (CI check).
- 🔒 **CRA-2 (blocker)** — `cargo install cargo-nextrs-dev` fails (unpublished, `publish=false`). **Needs release/decision:** publish it or ship the watcher in the published crate.
- 🔒 **CRA-3 (high)** — scaffolder itself unpublished + undocumented. **Needs release:** publish `create-nextrs-app`; add README quickstart.
- 🔧 **CRA-4 (medium)** — manual workflow with a build-order trap. **Done:** ordered/annotated next-steps. **Remaining:** optionally have the scaffolder run the steps and/or write a project README + `.env.example`.
- ☐ **CRA-5 (low)** — scaffold `props.rs` hand-builds a seed vs. the example's typed companion. Align or add a pointer comment.
- ✅ **CRA-6 (info)** — verified: with `--nextrs-path` the full create→build→`cargo dev` flow works and matches current conventions.

## Documentation ↔ implementation drift
- ✅ **DOC-1 (blocker)** — MANIFEST Non-goals rewritten (dropped "no React/no JS bundle").
- ✅ **DOC-2/3 (high)** — Getting Started + Routing Conventions now cover `.tsx`/`props.rs` + a React track; removed "no client framework".
- ✅ **DOC-4 (high)** — README body: conventions table, Status, Project layout, test count (51→~121).
- ✅ **DOC-5 (high)** — React-server-props moved out of Roadmap, present tense, swc→rolldown.
- ✅ **DOC-6/7 (medium)** — ROADMAP marks scaffolder shipped; local-dev-workflow around `cargo-nextrs-dev`.
- ✅ **DOC-8 (medium)** — Deploy-Vercel guide adds React deploy reqs + the VRC fixes.
- ✅ **DOC-9 (medium)** — MANIFEST body (modules, DiscoveredRoute, tests) refreshed.
- ✅ **DOC-10 (low)** — streaming docs reframed as one of two rendering models.
- ✅ **DOC-11 (low)** — version pins normalized to 0.3.
- 🔒 **DOC-12 (decision)** — positioning (TanStack Router, "prefetch") outruns `main`; reconcile when the branches land.
