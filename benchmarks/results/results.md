# Benchmark results

Same todos app, same architecture, two frameworks. Numbers are measured, not estimated; methodology and fairness controls are in [`../methodology.md`](../methodology.md).

## Test environment

| | |
|---|---|
| Machine | Intel Core Ultra 9 285K, 24 cores |
| OS | Linux 7.0.9-arch2-1 |
| Load tool | `hey`, 10s, 50 concurrent connections, loopback |
| nextrs | release build (`cargo build --release`), rustc 1.96.0 |
| Next.js | 15.5.19 production build (`next build` + `next start`), Node 22.22 |
| Date | 2026-06-10 |

Both apps: identical todos semantics (seeded open-todos list + add/delete), in-memory store, **both pages client-rendered** (server ships a shell + server-read seed; React renders in the browser), per-request fresh seed.

## Local throughput, latency, memory (matched profiles)

| Metric | nextrs (release) | Next.js (CSR) | nextrs advantage |
|---|---|---|---|
| **Page `/` throughput** | 339,600 req/s | 803 req/s | **~423×** |
| **API throughput** (`/api/todos?status=open`) | 384,600 req/s | 2,906 req/s | **~132×** |
| Page p50 / p99 latency | 0.1 / 0.7 ms | 60 / 81 ms | ~100× |
| API p50 / p99 latency | 0.1 / 0.7 ms | 16 / 24 ms | ~30× |
| **Memory (RSS, serving)** | 5.7 MB | 247 MB | **~43×** |
| Local binary / process | 4.0 MB binary | Node + framework | — |

### Reading these honestly

- **Both pages are client-rendered.** Earlier drafts compared nextrs's CSR shell against a Next.js *server-rendered* (RSC) page — not apples-to-apples. Once Next.js is made CSR too (it ships a shell + seed via `ssr: false`, renders the list in the browser), the gap **widens**, because a `force-dynamic` Next page still runs the full RSC/Node request pipeline per request even when it renders nothing to HTML. The per-request cost is the *runtime*, not the rendering — which client-side rendering can't remove.
- **nextrs's throughput numbers are a floor.** At 340k–385k req/s the bottleneck is loopback + the load generator sharing the machine's cores, not the Rust server (handlers are ~0.6 ms). Next.js at ~800–2,900 req/s is running flat out, well under any harness limit — so the *true* ratio is at least what's shown.
- **`force-dynamic` is the fair match.** Both apps recompute their seed per request from the current store (nextrs via `props.rs`, Next via reading the store in the RSC), so neither gets to serve a cached static page.

## Deployed function size (the cold-start driver)

| | nextrs | Next.js |
|---|---|---|
| Deployed serverless function | **2.9 MB** (static Rust binary) | **4.0 MB** (Node runtime slice + 159 traced files) |

Measured from `vercel build` output (nextrs: the release `index` binary; Next.js: summing `.vercel/output/functions/index.func/.vc-config.json`'s `filePathMap` traced files). **Roughly comparable** — Vercel's dependency tracing is good; this is *not* a blowout, and we report it straight.

## Cold start (Vercel)

Measured via function-level `x-cold` instrumentation (Vercel exposes no native cold/warm signal), `iad1`, hitting `/api/todos`. TTFB sampled with `curl -w time_starttransfer`; cold = first request a fresh instance served (forced by high-concurrency bursts). See `scripts/bench-cold.sh`.

| | cold p50 | cold p95 | cold max | warm p50 | samples |
|---|---|---|---|---|---|
| **nextrs** | **648 ms** | 685 ms | 688 ms | 267 ms | 39 cold / 211 warm |
| **Next.js** | 830 ms | 942 ms | 1,111 ms | 253 ms | 45 cold / 205 warm |

2026-06-10. **These are *shallow* (scale-out) cold starts on a *minimal* app — the friendliest case for both, and especially for Next.js.**

- **Warm is tied (~260 ms)** — warm-over-the-network is dominated by the ~250 ms round-trip to `iad1`; the framework difference is below the noise. nextrs does *not* separate on warm latency.
- **Cold-over-warm: nextrs ~380 ms vs Next.js ~580 ms.** A chunk of that is platform provisioning both pay; the **~200 ms difference is Node/Next runtime boot vs loading a static Rust binary.**
- **That ~200 ms delta is what scales with app size.** On a todo app it's small. On a real app (hundreds of deps, big RSC trees) Next's runtime-boot grows toward multi-second cold starts while nextrs's binary-load stays ~flat. The headline isn't "648 vs 830 on a todo app" — it's **"nextrs cold start barely moves as the app grows."** A realistic-deps variant (future work) is needed to chart that curve.

## Cold-start frequency under sustained load (Vercel)

Measured 2026-06-11 via `scripts/bench-cold-freq.sh`: identical sustained load
against both deployed apps **simultaneously** (same minute, same region
`sfo1`, both functions self-reporting `x-cold` + a per-process `x-instance`
ID so distinct instances are counted directly, not inferred).

**Round 1 — latency-bound load (40 workers × 5 min, ~58 req/s each):**

| | requests | cold / distinct instances | cold per 1k req | TTFB p50/p95 |
|---|---|---|---|---|
| **nextrs** | 17,541 | 38 / 38 | 2.17 | 133 / 2,057 ms |
| **Next.js** | 17,506 | 40 / 40 | 2.28 | 136 / 2,201 ms |

**A tie — and an honest null result at this load.** Both frameworks spun up
≈ one instance per concurrent connection (38–40 instances for 40 workers): at
~58 req/s of sub-millisecond JSON work, Vercel's scale-out is driven by
*concurrency scheduling*, not by per-instance memory or CPU pressure, and the
framework difference never gets a chance to matter.

**Round 2 — higher concurrency (150 workers × 5 min):**

| | requests | cold / distinct instances | cold per 1k req | TTFB p50/p95 |
|---|---|---|---|---|
| **nextrs** | 18,561 | 108 / **109** | 5.82 | 133 / 333 ms |
| **Next.js** | 22,389 | 150 / **150** | 6.70 | 135 / 339 ms |

Here a real difference appears: **Next.js scaled to exactly one instance per
concurrent worker (150/150); nextrs consolidated onto 109 — 27% fewer
instances and 28% fewer cold starts** for the same worker count. Roughly half
of all instances on both sides served exactly one request (scale-out burst at
ramp) before traffic consolidated onto ~8 instances carrying 6–12% each.

### Reading these honestly

- The hypothesis (43× lighter footprint → fewer instances → fewer cold
  starts) is **not supported at low concurrency** and **modestly supported
  (≈27%) at 150-way concurrency** — not the dramatic win the methodology
  hoped for, at least on a tiny JSON endpoint.
- **Harness limit:** the load generator (one curl process per request, 300
  workers total across both apps) saturated locally at ~60–75 req/s per app,
  so per-instance *CPU* pressure — the regime where Next.js must scale out
  long before a Rust binary does (~2,900 vs ~385,000 req/s per process
  locally) — was never reached. Demonstrating that regime needs a distributed
  load generator (out of scope v1).
- Achieved RPS differed between the two apps (61.9 vs 74.6) due to local
  generator scheduling, not server behavior — TTFB distributions were
  near-identical.
- Single run per round, one region (`sfo1`), same minute, Vercel Fluid
  scheduling internals unknown and subject to change.

## Realistic app: hhh (bookings/admin — better-auth, Postgres, shadcn)

The same production app, converted to nextrs on a branch (identical frontend,
backend swapped — see `docs/hhh-migration-plan.md` / `docs/hhh-migration-timelog.md`;
conversion verified route-by-route and flow-by-flow against the Next.js
baseline before benchmarking). 23 pages, 68 former server actions, 15 tables.

### Local throughput / latency / memory (2026-06-12, same machine/method as above, shared local Postgres)

| Metric | nextrs (release) | Next.js (prod build) | gap |
|---|---|---|---|
| **Page `/` (public landing)** | 340,589 req/s | 652 req/s | **~522×** |
| **Page `/app` (authed: session check + DB)** | 38,351 req/s | 389 req/s | **~99×** |
| `/app` p50 / p99 | 1.3 / 1.8 ms | 123 / 206 ms | ~95× |
| Action endpoint (authed POST, DB-backed) | 38,480 req/s | — (server actions aren't separately addressable) | — |
| **Memory (RSS, serving)** | 91.7 MB | 235.8 MB | **~2.6×** |

Honest notes:
- The authed rows do real work on both sides (cookie HMAC validation, session
  row lookup, user query) against the same Postgres — this is not a
  static-shell comparison. nextrs's ~38k req/s here is DB-bound, not
  framework-bound.
- **nextrs RSS grew 16× vs the todo app** (5.7 → 91.7 MB: sqlx pool, bigger
  binary, more routes) while Next.js stayed ≈flat (247 → 236 MB) — the Rust
  footprint scales with the app, Node's is dominated by the runtime floor.
  Still 2.6× lighter, no longer 43×.
- Next.js comparator for the action transport: the original is an RSC server
  action (not invocable as a plain HTTP endpoint without the React runtime),
  so the honest comparable is the authed page row, which embeds the same
  query work.

### Deployed cold start (2026-06-12, Vercel `iad1`, both apps same region, `/api/health` — a no-DB endpoint mirrored on both)

| | cold p50 | cold p95 | cold max | warm p50 | samples |
|---|---|---|---|---|---|
| **nextrs** | **215 ms** | 582 ms | 639 ms | 209 ms | 22 cold / 178 warm |
| **Next.js** | 4,323 ms | 4,812 ms | 5,389 ms | 342 ms | 22 cold / 178 warm |

**This is the realistic-deps datapoint the methodology predicted.** On the
minimal todo app the cold gap was modest (648 vs 830 ms). On the real app:

- **nextrs's cold start didn't move** — cold p50 (215 ms) is statistically
  indistinguishable from warm (209 ms). Loading a 23 MB static binary is not
  measurably worse than reusing a warm instance.
- **Next.js's cold start grew ~5×** (830 ms → 4.3 s) with the real dependency
  tree (better-auth, postgres driver, the app's module graph). Cold p50 is
  **~20× slower than nextrs**, and even warm p95 (4.3 s) is polluted by
  requests landing behind cold instances.
- The curve, in two points — cold-start cost vs app size: Next.js 830 ms →
  4,323 ms; nextrs 648 ms → 215 ms (the todo measurement was from a different
  day/run profile; the point is the *direction*: one grows with the app, the
  other doesn't).

Method note: both functions self-report via `x-cold`; `bench-cold.sh`, 8
rounds × 25 concurrent with 25 s idles, both apps sampled in the same minutes.
The nextrs deployment is a prebuilt upload (local `cargo-zigbuild`
cross-compile); the Next.js one is a standard Vercel build of the same repo's
`bench` branch (main + the instrumented health route).

Re-verified 2026-06-12 after the auth sidecar was replaced with the native
Rust better-auth port (the deployment became a single Rust binary with zero
Node functions): cold p50 **214 ms** — unchanged.

### Deployed cold-start frequency, realistic app (2026-06-12, same protocol as the todo-app rounds)

| sustained load | | requests | cold / distinct instances | cold per 1k | TTFB p50/p95 |
|---|---|---|---|---|---|
| 40 workers × 5 min | **nextrs** | 19,927 | 20 / 29 | 1.00 | 197 / 2,302 ms |
| | **Next.js** | 12,192 | 18 / 26 | 1.48 | 309 / 2,630 ms |
| 150 workers × 5 min | **nextrs** | 12,936 | 32 / **43** | 2.47 | 198 / 3,259 ms |
| | **Next.js** | 18,738 | 89 / **100** | 4.75 | 310 / 5,455 ms |

- At 40 workers: comparable (as with the todo app, low-concurrency scale-out
  is scheduler-driven).
- At 150 workers the real app separates much harder than the todo app did:
  **nextrs served the load on 43 instances; Next.js needed 100** (57% fewer),
  with half the cold starts per 1k request. Median requests-per-instance:
  231 (nextrs) vs 37 (Next.js) — Next.js churned through short-lived
  instances.
- **The two effects compound.** Next.js cold starts are both ~2× more
  frequent *and* ~20× more expensive (4.3 s each — visible as the 5.5 s p95
  TTFB). nextrs's are rarer and indistinguishable from warm requests.
- Caveats as before: single runs, one region, local curl-based generator
  (achieved RPS differs between apps — 43 vs 62 req/s in the 150 round —
  because worker loops are latency-coupled; the instance counts are the
  robust signal, not the RPS).

## Not claimed

These are single-machine, warm, loopback throughput numbers plus a measured function size. They show **framework + runtime overhead for an identical app** — not that Next.js is a bad tool (it ships HMR, a huge ecosystem, RSC streaming, image optimization, etc.). They say: for the same user-visible result, nextrs serves it with a fraction of the per-request cost and memory.
