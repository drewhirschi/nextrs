# Cold starts vs. arrival pattern — experiment design (follow-up)

- **Date:** 2026-07-23
- **Status:** designed, not started
- **Context:** investigation of the landing page's "how often do you pay a cold
  start" section, where hhh-rs (76.3% burst cold rate, api) reads slightly
  worse than hhh-nextjs (70.1%) while the todos pair is lopsided the other way
  (react-todos 50.8% vs nextjs-todos 99.9%).

## What the investigation established (2026-07-23)

Live 12-concurrent bursts against the health endpoints, using `boot_id` as
ground truth for instance identity:

| App | Instances for 12 reqs | Behavior |
|---|---|---|
| hhh-nextjs | 6 | Fluid in-instance concurrency: one Node instance served 2–3 overlapping requests |
| nextjs-todos | 12 | No sharing at all — Fluid concurrency effectively off on that project |
| hhh-rs | 11 | No concurrent sharing (vercel_runtime serves one request at a time); one *sequential* reuse |

Conclusions:

1. hhh-rs's higher burst cold rate is a **runtime concurrency asymmetry**, not
   a measurement bug: Node overlaps concurrent requests during IO waits (the
   event loop serializes CPU work — hence hhh-nextjs's SSR page shows 98%,
   no better than Rust), while the Rust runtime spawns an instance per
   in-flight request.
2. The todos comparison is currently **unfair to Next.js**: nextjs-todos lacks
   the Fluid concurrency hhh-nextjs has. Fix before accruing new comparison
   data (likely a project-level dashboard toggle on `bench-nextjs-todos`;
   check Settings → Functions, CLI path unknown).
3. A **simultaneous burst is the one arrival pattern where response speed
   cannot reduce cold starts** — everyone needs max instances at t=0. Rust's
   path to fewer colds is: finish fast → instance free → absorb the next
   arrival sequentially. Real traffic has spacing; the pinger's burst doesn't.
4. On the composed metric — expected cold tax per request
   (cold_rate × cold_p50) — nextrs wins everywhere today:
   hhh pair api: 0.763 × 1215ms ≈ 927ms vs 0.701 × 4160ms ≈ 2916ms.

## Landing page reframe (do first, independent)

Replace / augment the "how often" section with **expected cold tax per
request** (burst_colds/burst_requests × cold_p50 — both already in the
`/api/coldstarts` aggregates; compute in `site/app/page.tsx`). Footnote the
concurrency asymmetry honestly: Node's in-instance concurrency reduces how
*often* a stampede pays a cold start; nextrs makes each cold ~4× cheaper, and
the section's current framing overstates the frequency win (the hhh api row
quietly contradicts it).

## The experiment

**Question:** do you get fewer cold starts with Rust under realistic
(spaced) arrivals, not just cheaper ones?

**Traffic shapes** — one generated schedule per trial, applied identically to
both apps of a pair, interleaved side-by-side:

1. **Simultaneous** — N at t=0 (today's burst; keep as worst-case anchor).
2. **Ramp** — N spread evenly over T (e.g., 20 over 5s).
3. **Poisson** — exponential inter-arrivals, λ swept (e.g., 2/s, 5/s, 10/s
   for ~15s): organic traffic hitting a cold app.

**IO-gating probe:** add a `?delay=100` variant (awaited 100ms sleep) to both
comparators' health endpoints (nextjs-todos in this repo,
`benchmarks/apps/nextjs/app/api/health/route.ts`; hhh pair in the hhh repo
bench branch; nextrs side likewise). Prediction if Fluid overlap is IO-gated:
on the delay endpoint Node's instance count stays flat as λ rises while
Rust's grows linearly; on the no-delay endpoint Rust's speed keeps its
instance count near 1 at moderate λ.

**Metrics per (app, shape, rate):**
- distinct `boot_id`s per 100 requests — instances spawned (ground truth,
  already in every record)
- cold rate, and expected cold tax per request (rate × cold_p50)
- latency p50/p95 *including queue time* — catches requests parked behind a
  4s Node boot

**Cold-baseline discipline:** shapes only measure cold behavior on a cold
fleet, and idle-decay is opaque. Rotate **one shape per 2-hour pinger run**
(`run_number % shapes`) so every trial inherits the naturally cold fleet and
each shape accumulates over days. Prefix each trial with one probe request;
if it returns warm, tag the trial (`extra.warm_start: true`) so analysis can
segment rather than discard.

**Plumbing:** sender-side only — extend `metrics/ping.mjs` (or a sibling
`metrics/shapes.mjs`) with the schedule generator + rotation. Records flow
through the existing POST → Turso pipeline unchanged (`telemetry_version: 4`,
`extra.shape`, `extra.offset_ms`, `extra.trial_id`, `extra.warm_start`).
Region-match fairness logic carries over as-is. Analysis: instances-per-100
and cold-tax curves vs arrival rate, per runtime — a docs-site page or
notebook once a few days accrue.

**Honest prediction:** simultaneous → Node wins frequency, Rust wins tax.
Poisson at moderate λ → Rust should win both (a 76ms handler absorbs
~13 req/s per instance serially; every Node cold locks an instance for 4+s).
If the data agrees, the landing claim becomes: under real traffic, fewer
*and* cheaper; under a stampede, more frequent but ~4× cheaper.

## Follow-up checklist

- [ ] Flip Fluid concurrency on `bench-nextjs-todos` to match `hhh-next`
      (dashboard; verify with a 12-burst boot_id check afterwards)
- [ ] Reframe landing section around expected cold tax per request + footnote
- [ ] `?delay` variants on all four comparator health endpoints
- [ ] Shape scheduler + rotation in the pinger; telemetry_version 4
- [ ] Curve analysis view after a few days of accrual
- [ ] (Related, bigger) investigate concurrent serving in `vercel_runtime` —
      axum is capable; the serialization is the runtime shim. If Rust gets
      in-instance concurrency, it wins frequency and cost outright.
