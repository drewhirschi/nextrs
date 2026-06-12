+++
title = "Why nextrs over Next.js"
description = "Same app, same UI, same database — measured head to head, twice: a minimal app and a real production app converted end-to-end"
section = "Why nextrs"
order = 0
+++

<div class="not-prose my-8 grid grid-cols-1 sm:grid-cols-2 gap-4">
  <div class="stats shadow w-full">
    <div class="stat">
      <div class="stat-title">Authed, DB-backed page</div>
      <div class="stat-value text-primary">99×</div>
      <div class="stat-desc">throughput vs Next.js — real app, same Postgres</div>
    </div>
  </div>
  <div class="stats shadow w-full">
    <div class="stat">
      <div class="stat-title">Public page render</div>
      <div class="stat-value text-primary">522×</div>
      <div class="stat-desc">340k req/s vs 652 req/s</div>
    </div>
  </div>
  <div class="stats shadow w-full">
    <div class="stat">
      <div class="stat-title">Cold start, real app</div>
      <div class="stat-value text-primary">20×</div>
      <div class="stat-desc">215 ms vs 4.3 s — and nextrs cold ≈ its warm</div>
    </div>
  </div>
  <div class="stats shadow w-full">
    <div class="stat">
      <div class="stat-title">Memory serving</div>
      <div class="stat-value text-primary">2.6×</div>
      <div class="stat-desc">92 MB vs 236 MB — real app, RSS</div>
    </div>
  </div>
</div>

Benchmark blog posts usually compare a hello-world. We did that too — but then we took a **real production app** (a bookings/admin platform: better-auth, Postgres, S3, shadcn/radix, 23 pages, 68 server actions) and converted it to nextrs with **byte-identical frontends** — same React components, same flows, verified route-by-route and flow-by-flow against the original before any benchmark ran. Only the backend changed: the Node/RSC runtime became a single compiled Rust binary.

Everything below is measured, reproducible from [`benchmarks/`](https://github.com/drewhirschi/nextrs/tree/main/benchmarks), and reported with its caveats. The conversion itself is documented down to per-slice timings in [`docs/hhh-migration-timelog.md`](https://github.com/drewhirschi/nextrs/blob/main/docs/hhh-migration-timelog.md).

## The real app, head to head

Local, matched profiles (release Rust vs production `next build`), same machine, same Postgres, `hey` with 50 concurrent connections:

| Metric | nextrs | Next.js | gap |
|---|---|---|---|
| **Page `/` (public landing)** | 340,589 req/s | 652 req/s | **~522×** |
| **Page `/app` (authed: cookie HMAC + session row + user query)** | 38,351 req/s | 389 req/s | **~99×** |
| `/app` latency p50 / p99 | 1.3 / 1.8 ms | 123 / 206 ms | ~95× |
| **Memory (RSS, serving)** | 91.7 MB | 235.8 MB | **~2.6×** |

The authed row is the one to stare at: both sides validate the session cookie and hit the same Postgres on every request. That's not a static-file trick — it's the per-request cost of the framework runtime, and it's two orders of magnitude.

The minimal-app numbers (same todos app, both client-rendered, in-memory store) are the ceiling: **~423×** page throughput, **~132×** API throughput, **~43×** memory (5.7 MB vs 247 MB). Details in [`benchmarks/results/results.md`](https://github.com/drewhirschi/nextrs/blob/main/benchmarks/results/results.md).

## Why it's this lopsided

A nextrs request is a **compiled Rust function** — the handler runs in well under a millisecond, with no per-request runtime to spin up. A Next.js request, even for a client-rendered page, runs through the **Node + React Server Components pipeline** every time: serialize the flight payload, resolve the dynamic import, run the framework's request machinery. The per-request cost is the *runtime*, not the rendering — which is why the gap holds even when both pages render in the browser.

## Cold starts: latency *and* frequency

Vercel exposes no cold/warm signal, so both apps self-report (`x-cold`, `x-instance` headers) and we count instances directly. Same region, both apps loaded **simultaneously**.

**Latency — this is where app size decides everything.** On the minimal app the gap is modest: cold p50 **648 ms vs 830 ms**, a ~200 ms difference that is Node runtime boot vs loading a static binary. On the **real app**, that boot cost explodes with the dependency tree:

| Cold start, real app (`iad1`) | nextrs | Next.js |
|---|---|---|
| cold p50 | **215 ms** | **4,323 ms** |
| cold p95 | 582 ms | 4,812 ms |
| warm p50 | 209 ms | 342 ms |

nextrs's cold start is statistically indistinguishable from its warm requests — loading the binary costs nothing your users can see. Next.js's grew ~5× from the todo app to **4.3 seconds**, because every cold instance re-boots the framework plus the app's module graph. One line grows with your app; the other doesn't.

**Frequency** — how often users actually *hit* a cold start. At low concurrency it's a tie, and we say so: Vercel scales per concurrent connection regardless of framework. Under 150-way sustained load on the **real app**, **Next.js needed 100 instances (89 cold boots); nextrs served the same load on 43 (32)** — 57% fewer instances, half the cold starts per request, and instance-time is what Fluid compute bills. The two effects compound: Next.js's cold starts are both ~2× more frequent *and* ~20× more expensive, which is why its p95 TTFB under that load was 5.5 s while nextrs's p50 sat at ~200 ms.

## The conversion is real — and repeatable

The real-app comparison only counts because the two frontends are identical. The conversion that got us there is codified in an agent-followable guide ([`docs/migrating-nextjs-to-nextrs.md`](https://github.com/drewhirschi/nextrs/blob/main/docs/migrating-nextjs-to-nextrs.md)): server actions become same-signature fetch shims (call sites unchanged), server-component pages become seeded client pages, and even better-auth moved into the binary — a native Rust implementation of its wire protocol (scrypt, signed session cookies, Google OAuth), oracle-diffed 48/48 against the real thing and locked in by 111 tests, with the unchanged better-auth React client none the wiser. The whole conversion was verified route-by-route, three roles, money flows step-by-step, plus a byte-level wire audit that caught two serialization drifts before they could ship.

The deployed nextrs app is **one Rust binary and a folder of static files**. No Node runtime anywhere.

Scaffold to fully-verified conversion: **~4.5 hours wall clock**, mostly parallel agents. The timelog has every slice.

## Reading it honestly

- **Warm latency over the network is a tie.** ~260 ms round-trips bury a sub-millisecond handler. nextrs wins throughput, memory, cold start, and instance count — not warm wall-clock latency.
- **nextrs's memory advantage shrinks as the app grows** — 43× on the todo app, 2.6× on the real one (5.7 → 92 MB; the sqlx pool and a 31 MB binary are real). Node's footprint barely moved (247 → 236 MB): it's dominated by the runtime floor, nextrs's by what your app actually uses.
- **Throughput numbers are floors.** At 340k req/s the load generator is the bottleneck, not the server.
- **This isn't "Next.js is bad."** Next.js ships HMR, a vast ecosystem, RSC streaming, image optimization — far more than these apps exercise. The claim is narrow: for the same user-visible app, nextrs serves it with a fraction of the per-request cost, memory, and cold-start exposure.

## Reproduce it

```sh
# Minimal app: throughput + memory (local)
benchmarks/scripts/bench-local.sh
# Real app: throughput + memory (local, DB-backed)
benchmarks/scripts/bench-hhh-local.sh
# Cold start latency + frequency (against deployed URLs)
benchmarks/scripts/bench-cold.sh      https://your-app.vercel.app/api/health
benchmarks/scripts/bench-cold-freq.sh https://your-app.vercel.app/api/health 300 40
```

Fairness controls — matched build profiles, both pages client-rendered, per-request fresh data, same-region simultaneous cold-start runs — are documented in [`benchmarks/methodology.md`](https://github.com/drewhirschi/nextrs/blob/main/benchmarks/methodology.md).
