# nextrs vs Next.js benchmarks

The same todos app — seeded list + add/delete, identical semantics, same architecture (client-rendered page with a server-read seed) — built two ways, measured head to head. Every number is measured and reproducible; the fairness controls and caveats are in [`methodology.md`](methodology.md).

## Headline (2026-06-10, Intel Ultra 9 285K · Vercel `iad1`)

| Metric | nextrs | Next.js | gap |
|---|---|---|---|
| **Page throughput** (rps) | 339,600 | 803 | **~423×** |
| **API throughput** (rps) | 384,600 | 2,906 | **~132×** |
| **Memory** (RSS, serving) | 5.7 MB | 247 MB | **~43×** |
| Cold start (p50, minimal app) | 648 ms | 830 ms | ~1.3× |
| Deployed function size | 2.9 MB | 4.0 MB | ~1.4× |
| Warm latency (deployed) | ~267 ms | ~253 ms | tied (network-bound) |

Throughput and memory are the crushing, machine-measured wins. Cold start and function size are modest on this minimal app — but nextrs's cold start is a *static binary load* that stays ~flat as an app grows, while Next.js's *runtime boot* climbs with every dependency (the realistic-app cold-start curve is future work). Warm latency over the network is dominated by geography, not the framework, for both.

Full numbers: [`results/results.md`](results/results.md).

## TODO — planned experiments (each another potential win)

- **Cold-start *frequency*** — nextrs's ~43× lighter footprint should let many more concurrent invocations share one warm instance, so Vercel scales out (cold-starts) less often *and* costs less. Preliminary burst data hints at it (15.6% vs 18.0% cold rate) but isn't rigorous; the proper test counts distinct instances spun up under sustained load. See [`methodology.md`](methodology.md#planned-experiments-todo).
- **Realistic-deps cold-start curve** — heavier app variants to show Next.js's cold start climbing toward multi-second while nextrs's stays flat.

## The two apps

- **nextrs** — [`../examples/react-todos`](../examples/react-todos) (React `page.tsx` + `prefetch.rs` server-seeded React Query cache). Deployed as the `nextrs-react-todos` project → [nextrs-react-todos.vercel.app](https://nextrs-react-todos.vercel.app).
- **Next.js** — [`apps/nextjs`](apps/nextjs) (Next.js 15 App Router, RSC seed → `ssr:false` client render, Route Handlers). Deployed as the `bench-nextjs-todos` project → [bench-nextjs-todos.vercel.app](https://bench-nextjs-todos.vercel.app).

This is the **small-app** pair; the **real-app** pair (`hhh-nextrs` vs `hhh-next`) and every other deployment are mapped in [`../docs/deployments.md`](../docs/deployments.md).

## Run it

```sh
# Throughput + memory (local; needs `hey`, node, cargo)
benchmarks/scripts/bench-local.sh

# Deployed function size (offline, from `vercel build` output)
benchmarks/scripts/bench-size.sh

# Cold start vs warm TTFB (against a deployed URL; functions self-report via x-cold)
benchmarks/scripts/bench-cold.sh https://<your-app>.vercel.app/api/todos?status=open

# Cold-start FREQUENCY under sustained load (cold per 1k requests, distinct
# instances via x-instance) — run against both apps simultaneously for fairness
benchmarks/scripts/bench-cold-freq.sh https://<your-app>.vercel.app/api/todos?status=open 300 40
```

## Deploying the apps (for the cold-start / deployed numbers)

- **Next.js** (`apps/nextjs`) — standard Vercel: `cd apps/nextjs && vercel deploy --prod`. Nothing special.
- **nextrs** (`../examples/react-todos`) — has Vercel-specific requirements (Rust toolchain pin, prebuilt bundle, runtime declaration, region-as-project-setting). They're documented in that example's [README "Deploy to Vercel" section](../examples/react-todos/README.md#deploy-to-vercel) — read it before deploying, the gotchas are non-obvious.

Both apps emit `x-cold` (first request a fresh instance served) and `x-instance` (per-process ID) response headers so `bench-cold.sh` can label cold vs warm and `bench-cold-freq.sh` can count distinct instances directly.

## What this is and isn't

It measures **framework + runtime overhead for an identical app** — not that Next.js is a bad tool (it ships HMR, a huge ecosystem, RSC streaming, image optimization, and far more than this app uses). The claim is narrow and honest: for the same user-visible result, nextrs serves it with a fraction of the per-request cost and memory, on a tiny static binary.
