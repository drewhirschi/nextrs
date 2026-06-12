# Plan: hhh-next → nextrs conversion + realistic-app benchmarks

**Status:** planned 2026-06-11. This is the working plan — keep it updated as
phases complete so progress survives session/context loss.

## Goal

The benchmark suite's "realistic-deps" experiment (methodology.md TODO #2)
needs a real app, not a todo demo. Drew's `hhh-next` (a booking/admin app:
~20 pages, better-auth, Postgres via kysely, S3 uploads, shadcn/radix UI) is
the candidate. Convert it to nextrs on a branch and benchmark the two
implementations against each other.

**Hard requirement: identical frontends.** The two variants must ship the same
features and the same user-visible UI — same React components, same flows.
They differ *only* in the backend (Next.js App Router / Node vs nextrs / Rust).
Any place the conversion forces a frontend change, that change gets backported
to the Next.js branch too, so the comparison stays backend-only.

## Where things live

- nextrs repo (this repo): the framework, the migration guide, benchmark
  scripts, results, this plan.
- `/home/drew/work/hhh-next`: the app. Conversion happens on branch `nextrs`
  there (create from current main; do NOT touch main).
- Time log: `docs/hhh-migration-timelog.md` in this repo (see below).

## Phases

### Phase 0 — Migration guide (before any conversion)

Write `docs/migrating-nextjs-to-nextrs.md`: an agent-followable, codified
procedure for converting a Next.js App Router app to nextrs. It should encode:

- Route inventory: how to map `app/**/page.tsx`, `layout.tsx`, `loading.tsx`,
  `app/api/**/route.ts`, middleware to nextrs conventions (`page.tsx` +
  `props.rs`, `route.rs`, `middleware.rs`, layouts).
- What stays byte-identical (client components, styles, public assets) vs what
  is rewritten (server components → client components + `props.rs` seeds,
  Route Handlers → Rust handlers).
- Backend service mapping: kysely/postgres → sqlx (or equivalent), better-auth
  → Rust session/auth implementation with the same cookie + endpoint contract
  (frontend auth client must keep working unchanged), S3 SDK → aws-sdk-rust,
  sharp → image crate, zod validation → serde + utoipa schemas.
- The build/bundle/deploy mechanics from the react-todos example (rolldown
  bundling, NEXTRS_SKIP_BUNDLE on Vercel, runtime pin, Cargo.lock pinning —
  see "deploy gotchas" in examples/react-todos/README.md).
- A per-route verification checklist (renders, data parity, auth state,
  mutations).

The guide is the deliverable that makes the conversion repeatable; the
hhh-next conversion is its first test.

### Phase 1 — Survey (subagent, read-only)

Explore `/home/drew/work/hhh-next`: full route inventory (pages, layouts, API
routes, middleware), data access per route, auth flows, external services,
env vars. Output: a conversion worksheet (route → what it needs) checked into
the `nextrs` branch as `MIGRATION.md`.

### Phase 2 — Convert (subagents, in slices)

On branch `nextrs` in hhh-next. Slice order (each slice = survey output unit,
delegated to a subagent, verified before the next):

1. Scaffold: Cargo workspace, nextrs app skeleton, build.rs, vercel.json,
   static/public wiring, CSS pipeline (tailwind/postcss output parity).
2. Unauthenticated pages (/, /about, /pricing, /contact).
3. Auth (better-auth replacement with same client contract) — the riskiest
   slice; do early, everything else depends on it.
4. DB layer (Postgres via sqlx; same schema, same queries).
5. User app section (/app/**: bookings, profile).
6. Admin section (/admin/**: classes, customers, products, schedule,
   instances, bookings).
7. API routes not covered above (avatar, uploads/S3, webhooks if any).

Main session = planning + review only; subagents do the work (keep context
clean). Each slice ends with: build green, routes verified, timelog updated.

### Phase 3 — Verification

- Side-by-side run (Next.js main vs nextrs branch) against the same Postgres.
- Walk every route logged-out, logged-in, admin: same content, same
  redirects, same mutations applied. Browser-driven checks (chrome-devtools
  MCP) + curl parity for APIs.
- Frontend diff check: client component sources should be identical or
  trivially-diffable between branches.

### Phase 4 — Benchmarks (the payoff)

**Runbook (drafted 2026-06-11 while Phase 3 ran):**

*Local half (no authorization needed):* `benchmarks/scripts/bench-hhh-local.sh`
(written) — both variants against the same local Postgres :5433, matched
profiles; pages `/` and `/app` (authed), the converted action transport,
RSS. Run after Phase 3 passes.

*Deployed half (needs Drew's go-ahead on each item):*
1. Drew's existing `hhh-next` Vercel project: **iad1**, Fluid, env: DATABASE_URL,
   BETTER_AUTH_SECRET/URL, Google OAuth, S3 (names checked via API; values not read).
2. Create `hhh-nextrs` project (root = the hhh repo on branch `nextrs`),
   same region **iad1**, copy env vars. The auth sidecar deploys as the
   sibling `api/auth.ts` function per the auth slice's vercel.json.
3. **DB decision for Drew:** benchmark load (sustained 5-min, 100+ instances
   each holding a sqlx/postgres-js pool) against the PROD DATABASE_URL risks
   connection exhaustion on his real DB. Options: (a) second database on the
   same host seeded via migrations + fixtures (recommended), (b) accept prod
   DB for the cold-start runs only and benchmark no-DB endpoints, (c) prod DB
   as-is (not recommended).
4. Cold-start instrumentation parity: nextrs variant already emits
   x-cold/x-init-ms/x-instance (api/index.rs). The Next variant needs the
   same markColdStart pattern added on a `bench` branch of main (one API
   route + optionally a no-DB /api/bench endpoint mirrored in the nextrs
   variant) — deployed to a third project (`hhh-bench-next`) or to hhh-next
   itself if Drew prefers.
5. Then: bench-cold.sh (latency) + bench-cold-freq.sh (frequency, both
   simultaneously) on the matched endpoints; bench-size.sh on both `vercel
   build` outputs; record the realistic-deps cold-start datapoint vs the
   todo-app numbers (methodology TODO #2's curve).

Run the full existing suite against both variants of the bigger app:

- Local: `bench-local.sh` (req/s, latency, RSS) — pages and APIs.
- Size: `bench-size.sh` (deployed function size).
- Cold-start latency: `bench-cold.sh` (deployed, both same region).
- Cold-start frequency: `bench-cold-freq.sh` (sustained load, distinct
  instances, cold per 1k requests).
- New for this app: pages-served-per-second on a DB-backed route, and the
  realistic-deps cold-start datapoint for methodology.md TODO #2 (todo-app
  cold start vs big-app cold start, both frameworks → the "Next.js cold start
  grows, nextrs stays flat" curve).

Record in `benchmarks/results/` with date + environment, same honesty caveats.

## Time tracking (per Drew)

Log how long each part of the conversion takes — it's data on migration cost.
File: `docs/hhh-migration-timelog.md` (this repo). One row per work unit:

| date | phase/slice | wall-clock | agent-time | notes (what was hard) |

Record: start/end timestamps of each phase and each subagent slice, plus a
note when something took disproportionately long (those notes feed back into
the migration guide).

## Risks / open questions

- **better-auth parity** is the long pole: session cookies, CSRF, the
  `[...all]` catch-all endpoints. Strategy: keep the better-auth *client*
  untouched and implement the server contract it expects in Rust. If that
  proves unreasonable, fall back to a shared minimal auth (cookie session)
  implemented identically in both variants — but then backport to the Next.js
  branch so the comparison stays fair.
- `sharp` image work and S3 presigning have direct Rust equivalents; verify
  output parity (bytes may differ, behavior must not).
- hhh-next uses bun (`bun.lock`) — build tooling differences are fine; only
  runtime behavior must match.
- nextrs feature gaps discovered mid-conversion (e.g. catch-all routes,
  cookies API, streaming) become framework issues to fix in this repo first.

## Current status / next action

- [ ] Phase 0: migration guide
- [ ] Phase 1: survey
- [ ] Phase 2: conversion slices
- [ ] Phase 3: verification
- [ ] Phase 4: benchmarks

(Separate, in-flight: todo-app cold-start *frequency* experiment — deploys
done, sustained-load run + results write-up pending; see methodology.md
"Planned experiments" #1.)
