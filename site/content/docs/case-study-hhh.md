+++
title = "Case Study: A Production Booking App in 6 Hours"
description = "A real gym-management app — Next.js to nextrs at full behavioral parity in one evening, then measured on every dimension"
section = "Case studies"
order = 21
+++

> The companion to the [1.37M-LOC port](/docs/case-study-port-at-scale): same
> scrutiny, opposite end of the size spectrum. A ~20k-LOC production
> booking/admin app (better-auth, Postgres, S3 avatars, shadcn/radix,
> drag-and-drop scheduling) converted to nextrs — **completely**, not
> structurally: 24 routes × 3 personas verified flow-by-flow, money flows
> step-identical, byte-level wire parity on representative endpoints.

## The conversion: ~6 hours, most of it parallel agents

From first survey to verified, benchmarked conversion took **one evening**
(~4.5 h wall-clock to code-complete-and-verified; ~6 h including the full
benchmark suite). The React frontend was kept byte-for-byte; the entire Node
backend — including all **68 React Server Actions** across 12 modules —
became typed Rust endpoints with same-signature TypeScript shims, so no
component noticed. better-auth was first bridged by a sidecar, then ported
natively (scrypt golden vectors, PKCE, OAuth state machine; 48/48 oracle
diffs against the live sidecar before deletion) — the deployed app is **one
Rust binary, zero Node functions**.

The conversion also *found* three latent bugs in the original: a real
overbooking race (fixed with `FOR UPDATE` + count-subquery in one
statement), a timezone dependency that made the JS test suite green only
under `TZ=UTC`, and a credit-FIFO path with no test coverage at all.
Porting is an audit.

## Dev loop: honestly, a wash at this size

Measured on both variants, same machine (20 cores / 31 GB — full table in
the repo's `benchmarks/results/hhh-devloop.md`):

| Dimension | Next.js 16 | nextrs |
|---|---|---|
| Production build (cold) | **7.5 s** → 36 MB `.next` | 68.8 s → **31 MB self-contained binary** |
| Type-check | tsc 1.1 s | cargo check 0.5 s (incremental) |
| Tests | 374 unit tests / 0.23 s (no DB) | 115 tests **incl. real-Postgres integration** / 1.2 s |
| Dev boot + first page | 3.1 s | < 0.1 s |
| Unseen page in dev | 0.2–0.5 s | ~0 ms |
| Dev-server RSS | **1,240 MB** | **14 MB** |

At 20k LOC, Turbopack is genuinely fast: sub-second page compiles, 3 s boot.
The large-app report's 7–22 s page opens are what this toolchain grows into
at 1.37M LOC; this app hasn't reached the pain. If your app is this size and
your dev loop is your complaint, nextrs is not the fix — apart from the two
orders of magnitude of dev-server memory.

## Runtime: not a wash at any size

The production gap does **not** wait for scale — measured live on Vercel,
same region, same minutes, both apps self-reporting cold starts:

| | nextrs | Next.js |
|---|---|---|
| Cold start p50 | **215 ms** (≈ its own warm: 209 ms) | 4,323 ms |
| Cold start p95 | 582 ms | 4,812 ms |
| Warm p50 | 209 ms | 342 ms |
| Instances to serve 150-worker load | **43** | 100 |
| Cold starts per 1k requests | 2.47 | 4.75 |
| Local throughput, public page | 340,589 req/s | 652 req/s |
| Local throughput, authed page + DB | 38,351 req/s | 389 req/s |
| Serving RSS | 91.7 MB | 235.8 MB |

The two effects compound: Next.js cold starts are ~2× more frequent *and*
~20× more expensive, surfacing as a 5.5 s p95 TTFB under load. nextrs's
cold start is statistically indistinguishable from a warm request — loading
a 31 MB static binary just isn't measurably worse than reusing a warm
instance. The same module-graph cost that dominates the big app's *dev loop*
dominates this small app's *production cold start*: it's one root cause with
two symptoms, and it's why the runtime gap shows up long before the dev-loop
gap does.

## Honesty ledger

- Dev-loop numbers are single runs on a 20-core box (the large-app report
  used a 24-core/125 GB machine; numbers aren't cross-comparable between
  reports). Runtime/cold-start numbers are from the 2026-06-12 measured
  rounds documented in the repo's `benchmarks/results/results.md`.
- The test suites aren't like-for-like: bun's 374 tests are pure in-process
  units; the Rust 115 include DB-backed engine tests with no bun equivalent.
- `next build` beats `cargo build --release` by ~9× at this app size, and
  Turbopack HMR beats the rebuild-and-reload loop. Stated plainly, same as
  the big report.
- Not verified in the conversion: Stripe webhooks, Google OAuth against live
  Google, SMTP (no creds in the bench environment).
- The Next.js baseline has 8 pre-existing tsc errors confined to test files;
  its app code type-checks clean.
