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

## The two apps

- **nextrs** — [`../examples/react-todos`](../examples/react-todos) (React `page.tsx` + `props.rs` server-seeded React Query cache).
- **Next.js** — [`apps/nextjs`](apps/nextjs) (Next.js 15 App Router, RSC seed → `ssr:false` client render, Route Handlers).

## Run it

```sh
# Throughput + memory (local; needs `hey`, node, cargo)
benchmarks/scripts/bench-local.sh

# Deployed function size (offline, from `vercel build` output)
benchmarks/scripts/bench-size.sh

# Cold start vs warm TTFB (against a deployed URL; functions self-report via x-cold)
benchmarks/scripts/bench-cold.sh https://<your-app>.vercel.app/api/todos?status=open
```

## What this is and isn't

It measures **framework + runtime overhead for an identical app** — not that Next.js is a bad tool (it ships HMR, a huge ecosystem, RSC streaming, image optimization, and far more than this app uses). The claim is narrow and honest: for the same user-visible result, nextrs serves it with a fraction of the per-request cost and memory, on a tiny static binary.
