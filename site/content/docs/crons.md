+++
title = "Cron Jobs"
description = "Declare schedules on protected Rust routes; nextrs generates the Vercel and Cloudflare trigger plumbing"
section = "Guides"
order = 13
+++

Vercel Hobby allows one imprecise cron per day. Cloudflare Workers' free tier
handles minutely schedules. nextrs lets you use both without leaving your app:
declare every schedule on its protected route, and the CLI generates the
trigger for the selected provider. Your logic always runs in your Rust app on
Vercel—the Cloudflare Worker (when one is generated) is a dumb trigger that
fetches your route with a bearer secret. Delete it and you lose nothing but
the schedule.

## Declare the protected route and schedule

A cron target is an ordinary API route with `#[nextrs::cron(schedule = "...")]`
in place of `#[nextrs::api]`. The macro is `api` plus the auth gate: both trigger
providers send `Authorization: Bearer $CRON_SECRET`, and the handler answers
with a structured 401 before body extraction unless the secret matches. If
`CRON_SECRET` is unset, it fails closed with a structured 503.

```rust
// app/api/cron/refresh/route.rs
use axum::http::StatusCode;
use axum::Json;

#[nextrs::cron(schedule = "0 6 * * *")]
pub async fn get() -> Result<Json<Report>, StatusCode> {
    // ... the actual work ...
}
```

Vercel is the default provider. For a subdaily schedule, generation warns that
Vercel Hobby supports only daily crons. Opt into Cloudflare's more flexible
free scheduling explicitly:

```rust
#[nextrs::cron(schedule = "*/10 * * * *", provider = "cloudflare")]
pub async fn get() -> Result<Json<Report>, StatusCode> {
    // ... the actual work ...
}
```

To keep a protected route ready without scheduling it yet, disable the
declaration while preserving its intended schedule:

```rust
#[nextrs::cron(schedule = "0 6 * * *", disabled = true)]
pub async fn get() -> Result<Json<Report>, StatusCode> {
    // ... the actual work ...
}
```

Disabled declarations are validated but omitted from generated Vercel and
Cloudflare schedules. Remove `disabled = true` to enable the trigger. Fresh
scaffolds use this form for the heartbeat example and include an empty
`CRON_SECRET` entry in `.env.example`.

Schedules are five-field UTC cron expressions. Scheduled handlers are GET
routes because both Vercel and the generated Cloudflare trigger send GET.

Delivery is at-least-once and imprecise, and redundant delivery from both
providers must be harmless — write handlers idempotently (compute a
deterministic time slot and tolerate redelivery rather than assuming exactly
one call per tick).

**Do the work foreground and let the status code tell the truth.** A cron has
no user waiting, so there is no reason to respond early: run the job inline
and return 200 only when it actually completed. The status code is your
delivery receipt — it lands in the Worker's log and your Vercel logs, so a
failing job shows up as a failing tick. Responding 200 immediately and
pushing the work into `WaitUntil` makes every tick report success even when
the job blew up. (Cost is a wash: Vercel bills active CPU, not wall clock.)
Reach for background execution only when the work can exceed the function's
execution window or needs retry semantics of its own.

## Generate and deploy

```bash
nextrs generate        # writes .nextrs/cloudflare/{worker.js,wrangler.toml},
                       # and .nextrs/vercel.json (with Vercel-provider crons)
nextrs cron deploy     # generate + `wrangler deploy` + sync CRON_SECRET
```

`cron deploy` discovers annotated routes, reads `CRON_SECRET`, deploys the Worker,
and stores the secret with it.

Credentials come from the process environment first. Anything unset there is
filled from env files in the app root — target-specific files before the
generic one, first hit wins per key:

1. `.vercel/.env.production.local` (what `vercel pull` / `vercel env pull` writes)
2. `.env.production.local`
3. `.env.production`
4. `.env.local`
5. `.env`

`nextrs deploy --preview` reads the `preview` files instead and never the
production ones. To be explicit, name the file (or a list) in `nextrs.toml`;
this replaces the search, and a missing file is an error:

```toml
[deploy]
env_file = ".vercel/.env.production.local"
```

Values are never printed — only how many variables were loaded. The full
reference is [Deploy env files](/docs/config#deploy-env-files).

It talks to Cloudflare one of two ways:

- **API-direct (no wrangler, no Node):** set `CLOUDFLARE_API_TOKEN` (an
  API token with the *Workers Scripts: Edit* permission) and
  `CLOUDFLARE_ACCOUNT_ID`. The CLI uploads the Worker with the secret as a
  binding and sets the schedules over HTTPS. This is the CI path.
- **wrangler:** with neither variable set, the CLI shells out to
  [wrangler](https://developers.cloudflare.com/workers/wrangler/) and uses
  its login (`wrangler login`). Convenient on a workstation.

Set the same `CRON_SECRET` on the Vercel project (`vercel env add
CRON_SECRET`) so the app can verify what the Worker sends.

Before touching Cloudflare, `cron deploy` runs a preflight: it fetches each
cloudflare-provider route at `app.url` **without** credentials and expects a
401. A 404 means the route isn't deployed there (wrong `app.url` or stale
deploy); a 200 means the route is missing its `#[nextrs::cron]` gate; unreachable
means the URL is wrong. Any of those aborts the deploy with the specifics —
`NEXTRS_CRON_SKIP_PREFLIGHT=1` overrides when you know better.

The generated `.nextrs/cloudflare/` directory is disposable — gitignore it and
regenerate on demand. Vercel-provider crons deploy with the app itself; the
Worker redeploys with `nextrs cron deploy` whenever schedules change.

Scaffolded apps ship with a disabled daily heartbeat starter. After setting
`CRON_SECRET`, remove `disabled = true` to generate its native Vercel trigger;
it needs no Cloudflare account. Use
`app/api/cron/heartbeat/route.rs` as the gated route to copy from. The
worked example `examples/react-todos` runs the same route every 10 minutes
through the Cloudflare shim.

See [dependencies](/docs/dependencies) for the full tooling list.
