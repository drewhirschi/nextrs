# Cloudflare cron shim — generous schedules without leaving Vercel

- **Status:** roadmap / not started (captured 2026-08-02)
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
