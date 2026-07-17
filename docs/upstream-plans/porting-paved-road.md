# Porting Has No Paved Road

- **Reported-in:** a real-world port (hand-assembled, app unnamed)
- **Date:** 2026-07-16
- **Status:** fixed in b40bb33 (docs guide) + 7bf1975 (AGENTS.md) + d7598fd (--adopt)

## Problem

A team porting an existing app to nextrs went "off-road": instead of starting
from `create-nextrs-app` output, they hand-assembled nextrs around their
existing auth/db/routes. They missed conventions (the `app/` tree contracts,
the generated `client/` package and its bare-import rule, the `cargo dev`
loop, prebuilt deploys) and hit avoidable confusion.

Root cause: there is no paved road for PORTS. The scaffolder only makes fresh
apps (`scaffold()` refuses a non-empty directory outright), and our porting
knowledge lives in narrative case studies
(`site/content/docs/case-study-port-at-scale.md`, `case-study-hhh.md`) and an
internal agent procedure (`docs/migrating-nextjs-to-nextrs.md`) — none of it
is an instruction page a porting team lands on, and none of it is machinery.

## Proposed Direction

Three fixes, one PR:

1. **Docs guide** — `site/content/docs/porting.md`: start from the scaffold
   even for a port and graft existing code INTO it; the strangler pattern for
   incremental conversion; the convention contracts a port must respect; a
   gotchas section (notably: `PrefetchConfig` controls only document-level
   Speculation Rules, not `/__nx/prefetch` data seeding or hover preload).
2. **Agent guardrails in the scaffold** — `create-nextrs-app` generates an
   `AGENTS.md` in every new app: conventions table, "never hand-roll what the
   scaffold generates", client/package.json bare-import rule, `cargo dev`,
   deploy script, and a "porting into this app" section pointing at the guide.
3. **`create-nextrs-app --adopt`** — generate the skeleton INTO an existing
   repo: minimal content (one page, no demo routes), never clobber existing
   files (skip + per-file created/skipped report), `src/main.rs.example` when
   `src/main.rs` exists, printed Cargo.toml dep lines instead of editing an
   existing manifest.

## Implementation Notes

- Fresh-app output must stay byte-identical; `--adopt` is a separate template
  set that reuses the fresh templates where content is shared.
- Docs nav and llms.txt/llms-full.txt are generated from
  `content/docs/*.md` frontmatter by `nextrs::docs::emit_docs` — adding the
  page with frontmatter registers it everywhere.
- Write the gotcha against the CURRENT `PrefetchConfig` name; the rename to
  `SpeculationConfig` is planned but not shipped.

## Validation

- `cargo test --workspace` (scaffolder gains adopt + fresh-snapshot tests).
- `cargo build -p site` with bundling ON; load `/docs/porting` locally and
  confirm it renders and appears in the sidebar + llms.txt.
- `create-nextrs-app <tmp>/fresh-app` output unchanged (byte-compare against
  pre-change output).
- `--adopt` against a fake existing repo (pre-seeded `Cargo.toml`,
  `src/main.rs`, a stray file): skip-report correct, nothing overwritten.
