+++
title = "Case Study: Porting a 1.37M-LOC Next.js App"
description = "A 205-route production Next.js app ported to nextrs — every dev-loop dimension measured, honestly, including the two Next still wins"
section = "Case studies"
order = 20
+++

> **The full report, with methodology and raw numbers, is served here:
> [nextrs vs Next.js — the dev loop, measured](/case-studies/port-at-scale.html).**
> This page is the summary. One machine, one app, same database, same day;
> every number from a reproducible harness. nextrs numbers are from the
> **debug** build — a conservative floor.

## The app

A production SaaS dashboard: **1.37M lines of first-party TypeScript**,
205 routes, 404 models / 360 enums, ~1,935 API procedure signatures, and a
~768k-LOC React UI. The port is structurally complete (100% of routes,
schema, and dispatch wired and type-checked; the React UI reused
byte-for-byte via zero-copy) and behaviorally partial (~40% of procedures
have real `sqlx` bodies; ~7% of the 22,680 backend tests converted, with 681
documented `PORT-GAP`s instead of fake greens).

## Headline numbers

| Dimension | Next.js 16 dev | nextrs | Edge |
|---|---|---|---|
| First open of an unseen page (median) | 7.1 s (≤22 s) | 3.6 ms | ~1,975× |
| …in a real browser | 18.7 s | 45 ms | ~400× |
| Warm page | 682 ms TTI | 2.2 ms | ~300× |
| Dev-server RAM (load-tested) | 16–27 GB | 46→75 MB | ~210–600× |
| Minimum RAM to run at all | ~14 GB (OOM below) | <48 MB | ~300× |
| Production build | 238 s → 5.7 GB | 46 s → 47 MB binary | ~5× / 120× |
| Type-check | tsc 117 s (12 GB heap) | cargo check 27.5 s | ~4× |
| ~1,200 DB-backed tests | vitest+Prisma ~141 s/shard | cargo-nextest 1.6 s | ~85× |
| Cold start (live on Vercel) | 11.2 s to first byte | app-init ~9 ms locally | — |
| React hot-edit (HMR) | **0.3–0.7 s ✅** | 2.5 s | **Next** |
| Lint | **Biome 3.8 s ✅** | clippy 17.7 s | **Next** |

Eleven dimensions favor nextrs; two favor the JS toolchain, stated plainly.
The trade: give up ~2 s on the cheapest loop (HMR) to erase 7–22 s on the
most expensive one, plus two orders of magnitude of memory.

## The recurring villain: the JS module graph

The same root cause dominates three symptoms. Next's **cold start (11.2 s
live on Vercel)** is Node resolving and evaluating an enormous import graph
before the first byte. The **test suite** spends 445 s of CPU on `collect`
(rebuilding that module graph per test file, per worker) versus 78 s
actually running assertions — the real Postgres queries are *not* the
bottleneck. And **dev-server memory** holds that graph resident: kernel-OOM
at 4 GB and 8 GB caps; needs ~14 GB to render a page. A compiled binary has
no module resolution at runtime — the graph was linked at build time.

## Does it scale?

The workspace was artificially inflated 158k → 278k LOC with handler-shaped
code and re-measured: the compile slope is **~24 ms per 1,000 LOC**, because
dependencies (63% of the work) compile once and cache. The schema crate —
the heaviest, most serial part — is 100% ported already; what remains is
cheaper-per-line leaf-crate logic. Projected full-port cold build: ~45 s
best case, ~2 min worst — versus `next build` at 238 s *today*.

## What it unlocks

- **~7 engineer-weeks/year across 20 devs** recovered from cold-page spinners alone.
- No more 32 GB-machine floor (~$0 vs $60–280/mo/dev of cloud dev boxes).
- One 47 MB function instead of per-route bundles creeping toward Vercel's
  250 MB ceiling — React ships as static JS on the CDN, not in the function.
- The foreclosed-today unlock: **a live preview env per open PR, and
  agent-scale dev** — dozens of full app instances per box. Impossible at
  27 GB per instance; trivial at 48 MB.

## Honesty ledger

The comparison's imperfections all run *against* nextrs: the 24-core
benchmark box flatters Next's heavier compile and memory; nextrs's numbers
are the understating debug build; the behavioral port is partial and says
so with counted, documented gaps. Where the JS toolchain wins (HMR, lint),
the report says that too. Full methodology in the
[complete report](/case-studies/port-at-scale.html).
