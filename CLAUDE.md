# nextrs — agent working notes

nextrs is a Rust (Axum + Askama) framework that brings Next.js-style file routing,
React/TSX pages, and server-seeded TanStack Query to Rust, deployable to Vercel as a
single serverless function.

## Workspace layout

- `crates/nextrs` — the framework crate (router, bundler, conventions, macros re-export).
- `crates/nextrs-macros` — proc-macros (depend on `nextrs`, not this directly).
- `crates/create-nextrs-app` — the `npx`-style scaffolder; **generates the app `main.rs`**
  and client. Its string templates are what every new app ships with.
- `crates/cargo-nextrs-dev` — the `cargo dev` / `nextrs-dev` runner.
- `site/` — the docs site (self-contained Vercel deploy; see below).
- `examples/react-todos` — worked example. Depends on published `nextrs`, patched to
  local source inside the workspace (see root `Cargo.toml` `[patch.crates-io]`).

## Working with upstream app feedback  ← the core workflow

Drew builds real apps on nextrs (onenote-extractor, hhh, …) and pastes back the issues
they surface. These are the framework's most valuable signal. Handle every one through
this loop so the progression stays traceable and we never fix the same thing twice.

### 0. Search first — "didn't we already fix this?"

**Before writing any code**, check whether we've seen it. Report what you find.

```bash
rg -il "<keywords>" docs/upstream-plans/          # is there a plan for it?
git log --oneline --grep "<keywords>" -i          # did we already commit a fix?
git log --grep "Reported-by: <app-name>"          # everything a given app taught us
```

If a plan exists with `Status: fixed in <hash>`, say so and point at the commit — don't
re-solve it. If the fix shipped in the framework but a **generated app** still has the old
behavior, the app's own `main.rs`/scaffolded code won't self-update: note that the fix
exists and offer to adopt it in that app.

### 1. Capture — one file per issue in `docs/upstream-plans/`

Each issue gets a Markdown file (Problem / Proposed Direction / Implementation Notes /
Validation) plus a header so we know where it came from and where it stands:

```markdown
# <Issue Title>

- **Reported-in:** onenote-extractor
- **Date:** 2026-07-01
- **Status:** open   <!-- open | fixed in <commit> | wontfix -->

## Problem
...
## Proposed Direction
...
## Implementation Notes
...
## Validation
<the tests/checks that prove the fix>
```

Add it to the `docs/upstream-plans/README.md` index.

### 2. Fix in atomic commits with linking trailers

One logical change per commit. The body explains symptom → root cause → fix, and git
trailers make history queryable:

```
fix(<area>): <what changed>

<app> hit <symptom>. Root cause was <cause>. This <fix>.

Upstream-plan: docs/upstream-plans/<slug>.md
Reported-by: <app-name>
```

- Use Conventional-Commit subjects (`fix(dev-runner):`, `feat(scaffold):`, …).
- `<area>` scopes: `framework`, `macros`, `scaffold`, `dev-runner`, `site`, `docs`.
- Always include the `Upstream-plan:` and `Reported-by:` trailers when the work came
  from app feedback — that is what powers the search in step 0.

### 3. Close the loop

When the fix lands, update the plan's `Status:` to `fixed in <commit-hash>`. Now the plan
doc and the commit point at each other, and `git log --grep`/the plan index both answer
"did we already fix this?".

## The demo app is the living reference (important)

`examples/react-todos` is the canonical "this is what it looks like when it's
implemented." **Every change to how the framework works must be reflected there
in the same PR** — new conventions, renamed conventions, new client helpers,
new codegen behavior. It stays deliberately small so a gap is easy to spot.
When adding a feature, ask: "where does react-todos demonstrate this?" — if
nowhere, the change isn't done. Verify by actually running it
(`cargo build -p react-todos && PORT=<free> ./target/debug/react-todos`), not
just compiling.

**`site/` is an app too.** It consumes the framework via a path dep and picks
up every change immediately — a convention change that isn't reflected there
ships straight to docs production on the next push to main. After framework
behavior changes, also `cargo build -p site` (with bundling ON — its
skip-bundle dev path hides bundling breakage) and load the landing page.
See docs/postmortems/2026-07-11-docs-site-dead-landing.md for what skipping
this costs.

## Framework vs. generated-app changes (important)

A fix in `crates/nextrs` reaches existing apps on their next `cargo update`/rebuild. A fix
in `crates/create-nextrs-app` templates only reaches **newly generated** apps — existing
apps keep the code that was baked into their `main.rs` at generation time. When both apply,
prefer putting reusable logic in the framework crate and having the scaffold call it, so
existing apps can adopt it with a one-line change.

## Deploy / conventions quick refs

- Docs site: project `nextrs-docs` (Vercel `ashirsc` team), auto-deploys on push to `main`.
  Post-2026-06-29 reorg, `site/` is self-contained — the Vercel project's **Root Directory
  must be `site`** with "Include source files outside the Root Directory" enabled.
- Full deploy topology and other cross-repo facts live in agent memory, not the repo.
