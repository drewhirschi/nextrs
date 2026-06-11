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

## Not claimed

These are single-machine, warm, loopback throughput numbers plus a measured function size. They show **framework + runtime overhead for an identical app** — not that Next.js is a bad tool (it ships HMR, a huge ecosystem, RSC streaming, image optimization, etc.). They say: for the same user-visible result, nextrs serves it with a fraction of the per-request cost and memory.
