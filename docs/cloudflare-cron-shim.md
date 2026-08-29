# Cloudflare cron shim — generous schedules without leaving Vercel

- **Status:** shipped in ba83462 (merge of feat/cloudflare-cron, 2026-08-22); published as nextrs 0.6.0 / cargo-nextrs 0.2.0 / create-nextrs-app 0.1.4. Verified end-to-end 2026-08-22: react-todos redeployed on 0.6.0, `nextrs cron deploy` shipped react-todos-cron to Cloudflare (after the 0.2.1 config-path fix), and the 21:50 UTC tick hit /api/cron/heartbeat with a 200 in the Vercel logs
- **Decisions (updated 2026-08-29):** schedules live directly on protected
  `#[nextrs::cron(schedule = "...")]` GET routes; Vercel is the default and
  Cloudflare is an explicit provider for flexible free scheduling. CLI owns
  the plumbing: `nextrs cron generate` (worker.js + wrangler.toml into
  `.nextrs/cloudflare/`, vercel-provider crons merged into vercel.json) and
  `nextrs cron deploy` (generate + `wrangler deploy` + `wrangler secret put
  CRON_SECRET`). Runtime gate is the macro-injected `CronAuth` extractor
  (Bearer CRON_SECRET, structured fail-closed rejection). Demo: react-todos
  `/api/cron/heartbeat` every 10 minutes. Docs: site /docs/crons +
  /docs/dependencies.
- **Follow-ups landed 2026-08-29 (branch feat/cron-followups):**
  `#[nextrs::cron]` macro (macros 0.1.8 / nextrs 0.6.1); `[vercel]` table in
  nextrs.toml renders the whole vercel.json (`nextrs generate`, /docs/config);
  Cloudflare-API-direct deploy when CLOUDFLARE_API_TOKEN + ACCOUNT_ID are
  set, wrangler otherwise; `nextrs deploy` = generate + prebuilt Vercel
  deploy + cron deploy (cargo-nextrs 0.3.0); scaffold ships nextrs.toml with
  a daily heartbeat cron and a `#[nextrs::cron]` route (create-nextrs-app
  0.1.6). Still deferred: jobs-sweep wiring (see background-jobs.md — Drew
  wants to be hands-on for that one).
- **Related:** [background-jobs.md](background-jobs.md) (jobs need a trigger; this is one)

## Why

Vercel Hobby allows one imprecise cron per day; Cloudflare Workers' free tier
has generous cron triggers (100k requests/day, plenty for hourly/minutely
schedules). Apps like finstream already work around this by hand: a ~10-line
Worker whose `scheduled()` handler fetches the app's cron endpoint with a
bearer secret. The Worker contains zero business logic — kill it and you've
lost nothing but a trigger — so it's pure plumbing the framework can own.

## Vision

The user connects a Cloudflare API key once, and nextrs handles the rest —
the same "declare it in one place, the plumbing exists" contract we have with
Vercel:

- **Declaration.** App declares schedules once. nextrs already knows about
  `vercel.json`'s `crons` array; extend the entry shape to something like
  `{ path, schedule, provider: "cloudflare" | "vercel" | "both" }`. Smart
  default: route fine-grained schedules to the CF shim, coarse (daily) ones
  to native Vercel crons, since Hobby caps at 1/day.
- **Generation.** A codegen step (same pattern as the existing route-registry
  generation) emits the Worker script + `wrangler.toml` into a build dir.
  The Worker is dumb by design: `scheduled()` → `fetch(app_cron_url,
  { Authorization: Bearer env.CRON_SECRET })`. All real logic stays in the
  Rust app.
- **Deploy + secrets.** Wrap `wrangler deploy` and `wrangler secret put
  CRON_SECRET` (reading from the same env the app uses), driven by the
  imported CF API key. Possibly go through the CF API directly instead of
  requiring wrangler — TBD.
- **Safe by default.** Require target routes to live under a
  cron-authenticated path (finstream's `is_cron` middleware gate is the
  reference), and document the idempotency contract: the caller is maximally
  dumb and delivery is at-least-once/imprecise, so endpoints must compute
  deterministic slots and tolerate redelivery (finstream's
  `SyncCadence::most_recent_slot` + DB unique index is the reference
  implementation). Redundant delivery from both providers should be
  harmless — Vercel's daily cron can stay on as a backstop.

## Why this doesn't create Cloudflare lock-in

The Worker is a trigger, not a runtime. Workers are V8 isolates (JS/WASM
only), so the app itself — native Rust, tokio, libsql — can't move there
anyway; this deliberately uses CF only for the one thing where their free
tier embarrasses Vercel's. The generated shim is disposable and regenerable.

## Open questions

- Wrangler CLI dependency vs. calling the Cloudflare API directly from
  `cargo nextrs` (the API-direct path avoids requiring Node/wrangler on the
  user's machine but means owning more surface).
- Where the CF API token lives (env var convention? `.nextrs` config?) and
  how it flows through CI/deploy.
- Whether the same shim mechanism generalizes to other trigger providers
  later (this is really "external cron provider" with CF as the first
  implementation).
- Interaction with [background-jobs.md](background-jobs.md): scheduled jobs
  are just cron-triggered jobs, so the declaration surface should probably
  be shared rather than two parallel systems.
