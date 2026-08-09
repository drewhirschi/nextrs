# Background jobs — first-party, built on WaitUntil

- **Status:** v1 implemented on `feat/background-jobs` (2026-08-04) — see below for what shipped vs. deferred
- **Builds on:** [waitUntil](upstream-plans/vercel-wait-until.md) (shipped in 0.4.1)

## What shipped in v1 (2026-08-04)

`app/jobs/<name>/job.rs` + `#[nextrs::job]`: calling the annotated fn from app
code is a typed *enqueue* (macro re-emits the name as a wrapper returning
`JobHandle`); the body runs at `POST /__nx/jobs/<name>` behind
framework-managed WaitUntil with per-attempt timeouts, exponential back-off
retries, and DB-backed rows (`JobStore` trait: in-memory dev store +
Turso/libsql behind `jobs-libsql`, auto-migrating `__nextrs_jobs`). Authed via
`NEXTRS_JOBS_SECRET` (fail-closed; per-process dev secret locally; fail-loud
on Vercel when unset). `/__nx/jobs/sweep` (cron-driven redelivery + stale
reclaim, accepts `Bearer $CRON_SECRET`) and `GET /__nx/jobs` (status JSON)
round it out. Demo: react-todos `audit-todo`. User docs:
site /docs/conventions#background-jobs.

Deferred (unchanged from the open questions): TS enqueue client, dashboard
UI, delayed enqueue, idempotency keys, Vercel Queues/workflow backend, the
[Cloudflare cron shim](cloudflare-cron-shim.md) as the sweep trigger.

## Why

Real apps end up wanting background jobs almost immediately — the recurring
asks are retries, back-off, and visibility into what ran and what failed.
Today the answer is "use `nextrs::WaitUntil` and roll your own," which gives
you fire-and-forget but none of the durability or observability.

## Vision

A first-party, type-safe jobs API:

- **Registration.** Apps register job functions with the framework — a macro
  is fine here if it buys ergonomics (mirroring how routes/api handlers are
  already convention + macro driven).
- **Type-safe invocation.** Calling a job is a typed call — the payload type
  is checked at the call site, same spirit as the typed client codegen. No
  stringly-typed job names + JSON blobs at the boundary.
- **Execution via WaitUntil.** Enqueueing a job is an HTTP call to our own
  backend (a framework-provided route, e.g. something under `/__nx/`). That
  handler looks up the registered job function by its registration and runs
  it in the background behind `wait_until`, so the enqueue request returns
  immediately and the work is kept alive by the platform's invocation
  lifecycle rather than a detached `tokio::spawn`.
- **Persistence (maybe).** Possibly write rows into the *user's* database
  representing each job — status, attempts, timestamps — which is what makes
  retries, back-off, and a jobs dashboard possible. Undecided whether this is
  core or an opt-in layer; needs thought about which DB conventions we'd be
  imposing.

## Vercel Queues as a backend (researched 2026-08-05 — promising, gated on a spike)

Docs reviewed: /docs/queues{,/concepts,/api,/pricing}. Drew's read: likely
much better than the DB backend on Vercel. Findings:

- **Fits**: at-least-once delivery with 3-AZ replication before publish
  returns; visibility-timeout leases + automatic redelivery (replaces
  claim/reclaim/sweep — and beats the sweep's cron-bounded retry latency);
  push-mode consumers are air-gapped (no public URL → our whole
  NEXTRS_JOBS_SECRET/base-URL machinery becomes unnecessary); delayed
  delivery ≤7d and idempotency keys (both were v2 wishes); per-group
  concurrency limits; Vercel-native observability (the "inherit Vercel
  observability" wish). **Full REST API** at
  `https://{region}.vercel-queue.com/api/v3` (Send/Receive/ReceiveById/
  Ack/ExtendLease), authed via `VERCEL_OIDC_TOKEN` — usable from Rust, no JS
  SDK needed.
- **Losses vs. DB rows**: no queryable job history (acked messages are gone
  — JobHandle.status()/GET /__nx/jobs die unless we add an optional audit
  write); no DLQ (app-level ack-and-drop after max attempts); everything
  expires ≤7d; Vercel-only (keep libsql as the portable backend).
- **Hobby availability: VERIFIED WORKING (2026-08-05).** Full REST
  round-trip executed against team ashirsc (Hobby) with the nextrs-docs
  project's `VERCEL_OIDC_TOKEN` (via `vercel env pull`), region pdx1:
  SendMessage → 201 + messageId; ReceiveMessages (ndjson, custom visibility
  timeout) → 200, payload intact; AcknowledgeMessage → 204. Public beta,
  $0.60/1M ops (no Hobby allotment documented — pricing during beta
  unclear, watch the usage page). The entire data plane works from plain
  curl/reqwest — no JS SDK.
- **Remaining risks (beta)**: push trigger is `experimentalTriggers:
  [{type: "queue/v2beta", topic, retryAfterSeconds}]` in vercel.json —
  every example is Next.js; **still unverified whether a vercel-rust
  function can carry it** (the linchpin: serverless Rust can't poll
  continuously; needs a deploy test). Callback wire format is undocumented
  (inferred: carries topic+messageId; consumer does ReceiveMessageById →
  run → Acknowledge). Topic names are `[A-Za-z0-9_-]` only (nested
  `email/welcome` job names need flattening). Local dev needs `vercel env
  pull` for the OIDC token (expires ~12h) — the in-memory dev backend
  stays the better local story.
- **Key architecture fact**: the v1 user surface (macro, job.rs convention,
  typed wrappers, JobHandle) is backend-agnostic — Queues would be a third
  backend behind the same API; local dev keeps the memory store.
- **Next step**: spike, not design — deploy a throwaway vercel-rust function
  with a queue/v2beta trigger, SendMessage via REST + OIDC, observe (a)
  whether it deploys, (b) the callback request shape. Those two answers
  decide default-backend status.

## Open questions

- **The Vercel-native path.** We're leaning hard into Vercel already, so
  before building our own durability layer, explore what we'd inherit for
  free: Vercel Queues (durable at-least-once delivery on Fluid Compute),
  Vercel's workflow/steps primitives, and their built-in observability. The
  ideal outcome is that nextrs jobs *are* Vercel-observable rather than us
  rebuilding dashboards. The trade-off is portability — WaitUntil already has
  a spawn-backed fallback for non-Vercel targets, and jobs should keep that
  shape (Vercel backend on Vercel, plain-tokio or DB-polling backend
  elsewhere).
- **Retry/back-off semantics.** Where do retries live if the execution
  primitive is a single invocation's `wait_until`? A failed job needs
  something to re-enqueue it (DB row + cron sweep? queue redelivery?).
- **maxDuration bound.** `wait_until` work holds the invocation open and is
  bounded by the function's `maxDuration` — a hung job keeps the invocation
  alive until timeout. Job execution needs its own internal timeout, and
  long jobs may need chunking/re-enqueue rather than one long invocation.
- **Where rows live.** If we populate the user's DB, we need a story for
  "which DB" (nextrs has no DB convention today) and migrations for the jobs
  table.

## Non-goals (for now)

- A general-purpose distributed task queue. The target is "the jobs a Vercel
  app actually needs" — webhooks, notifications, syncs — with retries and
  visibility, not Sidekiq/Celery parity.
