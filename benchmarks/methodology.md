# Methodology

How the numbers in [`results/results.md`](results/results.md) are produced, and what is and isn't claimed.

## The apps

Two implementations of the *same* todos app — seeded open-todos list, add, delete — with identical semantics, seed data, and visual output:

- **nextrs** — `examples/react-todos` (React `page.tsx` + `props.rs` server-seeded React Query cache).
- **Next.js** — `benchmarks/apps/nextjs` (Next.js 15 App Router).

Fairness controls baked in:

- **Both pages are client-rendered.** The Next.js page ships a shell + server-read seed and renders the list in the browser (`ssr: false`), exactly like nextrs's `<div id="__nx_root__">` + bundle. We do **not** compare nextrs's CSR shell against a server-rendered RSC page — that would charge Next.js for rendering work nextrs offloads to the client.
- **Per-request fresh seed.** Both recompute the seed from the in-memory store on every request (`force-dynamic` in Next, `props.rs` in nextrs), so neither serves a cached static page.
- **In-memory store both sides** — no DB, so we measure framework/runtime overhead, not I/O.
- **Matched build profiles** — nextrs release build, Next.js production build (`next build` + `next start`). Same machine, same load tool (`hey`), warm, back to back.
- **Idiomatic Next.js** — App Router + RSC + Route Handlers (its fast path), not a hand-nerfed config.

## Throughput / latency / memory

`hey -z 10s -c 50` against the page (`/`) and the API (`/api/todos?status=open`); RSS read from `/proc/<pid>/status` while serving. See `scripts/bench-local.sh`.

Honesty caveats (also in results):
- At 340k–385k req/s, **nextrs is bound by loopback + the load generator** sharing the machine's cores, not the server — so its throughput is a *floor*. Next.js at ~800–2,900 req/s is well under any harness limit, so the real ratio is at least what's shown.
- Single machine, warm, loopback. Not a distributed/real-network test.

## Deployed function size

Measured from `vercel build` output, the real deployed artifact:
- **nextrs** — the release `index` binary (the whole function).
- **Next.js** — summing the files in `.vercel/output/functions/index.func/.vc-config.json`'s `filePathMap` (the traced `node_modules` + build outputs Vercel assembles). A plain `du` of the local `.func` dir *under-reports* this — the deps are referenced, not bundled, until deploy. See `scripts/bench-size.sh`.

## Cold start (Vercel)

Vercel exposes **no native cold/warm signal** (`x-vercel-cache` is the CDN, `x-vercel-id` is request routing), so the function reports it itself:

- Each function records a process-start timestamp and a "first request on this instance" flag, and sets response headers:
  - `x-cold: 1|0` — was this the first request this instance served (i.e., it bore the cold-start cost)?
  - `x-init-ms` — ms from process/module start to handling this request (the function's own init contribution; the *full* cold cost is the client-observed TTFB).
- **Sampling** (`scripts/bench-cold.sh`): fire concurrent bursts with idle gaps. A burst against a deployment with no warm capacity makes Vercel spin up fresh instances, each returning `x-cold: 1` on its first response — so one burst yields several cold samples. The script records TTFB (`curl -w time_starttransfer`) per request, buckets by `x-cold`, and reports cold vs warm p50/p95.
- Measured on the **API endpoint** (`/api/todos`) for both — a clean function-to-function cold-start comparison (Node+Next runtime boot vs a static Rust binary). For nextrs the entire app is one function, so this *is* the page's cold start; for Next.js the page and API are separate functions with comparable runtime-boot cost.

Cold-start figures depend on Vercel internals and are reported as measured-on-a-date, same region (`iad1`), same function memory.

## What we don't claim

That Next.js is a bad tool — it ships HMR, a huge ecosystem, RSC streaming, image optimization, and far more than this app exercises. We claim only: for an identical user-visible app, nextrs serves it with a fraction of the per-request cost, memory, and (pending) cold-start latency, because the difference is a static Rust binary vs the Node/RSC runtime.

## Out of scope (v1)

Pure-HTML Rust bar, DB-backed variants, multi-region/edge, streaming-SSR latency curves, distributed (off-machine) load generation.
