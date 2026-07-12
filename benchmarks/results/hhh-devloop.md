# hhh dev-loop measurements (2026-07-12)

Companion to the runtime/cold-start numbers in `results.md` §"Realistic app:
hhh" — this measures the *developer loop* on both variants, mirroring the
dimensions of the large-app port report (docs site
`/case-studies/port-at-scale.html`).

Environment: 20 cores, 31 GB RAM (NOT the 24-core/125 GB box the large-app
report used), Linux, bun 1.3.5, Next 16 (Turbopack), nextrs 0.2.1 (the
version the branch pins), shared local Postgres 17 in Docker. Wall-clock,
single runs. The two variants are the same product with identical React
frontends (main = Next.js backend, `nextrs` branch = Rust backend);
conversion was verified route-by-route (24 routes × 3 personas) before any
benchmarking.

| Dimension | Next.js 16 (main) | nextrs (branch) | Note |
|---|---|---|---|
| First-party code | 20,064 LOC TS | 23,779 LOC TS (React reused + generated client/shims) + 14,578 LOC Rust | |
| Production build (cold) | 7.5 s → 36 MB `.next` | 68.8 s → **31 MB self-contained binary** | Next wins time; artifacts differ in kind (`.next` still needs node_modules to run) |
| Incremental rebuild | — (dev recompiles on demand) | 9.7 s (release relink after touching `main.rs`) | |
| Type-check | tsc 1.1 s warm (8 pre-existing errors in main's test files) | cargo check 0.5 s incremental / 27.4 s cold-cache | wash |
| Lint | (next lint configured) | clippy 3.4 s warm | wash |
| Test suite | 374 tests / **0.23 s** — pure in-process unit tests, no DB | 115 tests / **1.2 s** — *including* booking-engine integration tests against real Postgres | not like-for-like: the Rust suite's DB-backed tests have no bun equivalent |
| Dev boot + first page | 3.1 s | **< 0.1 s** | |
| First open, unseen page | 0.2–0.5 s | ~0 ms | Turbopack is genuinely fast at this size |
| Warm page | ~0 ms | ~0 ms | wash |
| Dev-server RSS | **1,240 MB** | **14 MB** (idle; 91.7 MB under load, from results.md) | ~90× |

## Reading these honestly

At 20k LOC the Next.js dev loop **does not hurt**: 3 s boot, sub-second page
compiles, instant HMR. The large-app report's 7–22 s page opens and 14 GB dev
floors are what this same toolchain grows into at 1.37M LOC — the pain is
scale-dependent, and this app hasn't reached it. Anyone selling "Next dev is
slow" on an app this size is overclaiming.

What is *not* scale-dependent — measured here at 20k LOC and in results.md
on Vercel — is the runtime story: cold p50 **215 ms vs 4,323 ms** (~20×),
warm-indistinguishable cold starts, **43 vs 100 instances** under the same
load, 522×/99× local throughput, and two orders of magnitude of dev-server
memory. The module-graph cost that dominates the big app's dev loop already
dominates this small app's production cold start.

Also measured on the way: schema drift between the branch's migrations and a
months-old dev volume (both suites need migration 015), and main's tsc has 8
pre-existing errors confined to test files. Recorded as found.

Raw logs: scratch `devloop-report.txt` / `devloop-rs.txt` from the run;
harness `bench-devloop.sh` (same method as `scripts/bench-hhh-local.sh`).
