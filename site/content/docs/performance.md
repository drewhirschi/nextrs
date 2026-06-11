+++
title = "Performance vs Next.js"
description = "The same app, two frameworks: nextrs serves it ~130–420× faster on ~40× less memory"
section = "Why nextrs"
order = 0
+++

We built the *same* todos app two ways — once with nextrs (React `page.tsx` + `props.rs` server-seeded cache) and once with idiomatic Next.js 15 (App Router, RSC seed, Route Handlers) — and measured them head to head. Same semantics, same architecture (a client-rendered page with server-read seed data), same machine, same load tool.

The full harness and methodology are in [`benchmarks/`](https://github.com/drewhirschi/nextrs/tree/main/benchmarks) — every number here is reproducible.

## The numbers

| Metric | nextrs | Next.js | gap |
|---|---|---|---|
| **Page render throughput** | 339,600 req/s | 803 req/s | **~423×** |
| **API throughput** | 384,600 req/s | 2,906 req/s | **~132×** |
| **Memory (RSS, serving)** | 5.7 MB | 247 MB | **~43×** |
| Deployed function size | 2.9 MB | 4.0 MB | ~1.4× |
| Cold start (p50, this app) | 648 ms | 830 ms | ~1.3× |

Measured 2026-06-10 on a 24-core machine (local throughput/memory) and Vercel `iad1` (cold start). `hey`, 50 concurrent, both apps in production/release builds.

## Why it's this lopsided

A nextrs request is a **compiled Rust function** — the handler runs in well under a millisecond, with no per-request runtime to spin up. A Next.js request, even for a client-rendered page, runs through the **Node + React Server Components pipeline** every time: serialize the flight payload, resolve the dynamic import, run the framework's request machinery. That per-request cost is the *runtime*, not the rendering — which is why the gap holds even when both pages offload rendering to the browser.

The memory story is the same shape: a static Rust binary serving from ~6 MB of RSS versus a Node process holding the framework runtime in ~250 MB.

## Reading it honestly

We measure, we don't spin:

- **Warm latency over the network is a tie (~260 ms).** It's dominated by the round-trip to the function's region, not the framework — nextrs's sub-millisecond handler is invisible next to ~250 ms of geography. nextrs wins on *throughput, memory, and cold start*, not warm wall-clock latency.
- **Throughput numbers are a floor.** At 340k req/s nextrs is bound by the loopback and the load generator, not the server — the real ceiling is higher.
- **Function size and cold start are modest on this minimal app**, and that's the honest result: a todo app has almost nothing to boot in either runtime. But nextrs's cold start is a *static binary load* that barely moves as an app grows, while Next.js's *runtime boot* climbs with every dependency — so on a real app the cold-start gap widens (a realistic-app curve is on our list).
- **This isn't "Next.js is bad."** Next.js ships HMR, a vast ecosystem, RSC streaming, image optimization — far more than this app exercises. The claim is narrow: for the same user-visible result, nextrs delivers it with a fraction of the cost.

## Reproduce it

```sh
# Throughput + memory (local)
benchmarks/scripts/bench-local.sh
# Cold start vs warm (against a deployed URL)
benchmarks/scripts/bench-cold.sh https://your-app.vercel.app/api/todos?status=open
```

See [`benchmarks/methodology.md`](https://github.com/drewhirschi/nextrs/blob/main/benchmarks/methodology.md) for the fairness controls.
