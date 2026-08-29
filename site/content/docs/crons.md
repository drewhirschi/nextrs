+++
title = "Cron Jobs"
description = "Declare schedules once in nextrs.toml; nextrs generates the Vercel and Cloudflare trigger plumbing"
section = "Guides"
order = 13
+++

Vercel Hobby allows one imprecise cron per day. Cloudflare Workers' free tier
handles minutely schedules. nextrs lets you use both without leaving your app:
declare every schedule in one place, and the CLI generates the right trigger
per provider. Your logic always runs in your Rust app on Vercel — the
Cloudflare Worker (when one is generated) is a dumb trigger that fetches your
route with a bearer secret. Delete it and you lose nothing but the schedule.

## Declare schedules in `nextrs.toml`

Create `nextrs.toml` at the app root:

```toml
[app]
name = "myapp"                        # names the generated Worker: myapp-cron
url = "https://myapp.vercel.app"      # where the triggers point

[[crons]]
path = "/api/cron/refresh"
schedule = "*/10 * * * *"

[[crons]]
path = "/api/cron/digest"
schedule = "0 6 * * *"
# provider = "cloudflare" | "vercel"  # optional override
```

When `provider` is omitted, nextrs routes by granularity: daily-or-coarser
schedules (fixed minute and hour) become native Vercel crons in `vercel.json`;
anything finer goes to the Cloudflare shim, since Vercel Hobby can't run it.

## Write the route

A cron target is an ordinary API route with `#[nextrs::cron]` in place of
`#[nextrs::api]`. The macro is `api` plus the auth gate: both trigger
providers send `Authorization: Bearer $CRON_SECRET`, and the handler answers
401 before its body runs unless the secret matches. The check is fail-closed
— if `CRON_SECRET` is unset, every request is rejected.

```rust
// app/api/cron/refresh/route.rs
use axum::http::StatusCode;
use axum::Json;

#[nextrs::cron]
pub async fn get() -> Result<Json<Report>, StatusCode> {
    // ... the actual work ...
}
```

The handler must return a `Result` whose error type accepts a `StatusCode`
(`StatusCode` itself or `nextrs::ApiError`). If you need the check somewhere
a macro can't reach, `nextrs::cron::authorize(&headers)` is the same gate as
a plain function.

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
nextrs cron generate   # writes .nextrs/cloudflare/{worker.js,wrangler.toml},
                       # merges vercel-provider crons into vercel.json
nextrs cron deploy     # generate + `wrangler deploy` + sync CRON_SECRET
```

`cron deploy` needs [wrangler](https://developers.cloudflare.com/workers/wrangler/)
plus a Cloudflare account: authenticate with `wrangler login`, or set
`CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` in CI. It reads
`CRON_SECRET` from the environment, deploys the Worker, and stores the secret
with it. Set the same `CRON_SECRET` on the Vercel project
(`vercel env add CRON_SECRET`) so the app can verify what the Worker sends.

Before touching Cloudflare, `cron deploy` runs a preflight: it fetches each
cloudflare-provider route at `app.url` **without** credentials and expects a
401. A 404 means the route isn't deployed there (wrong `app.url` or stale
deploy); a 200 means the route is missing its `authorize` gate; unreachable
means the URL is wrong. Any of those aborts the deploy with the specifics —
`NEXTRS_CRON_SKIP_PREFLIGHT=1` overrides when you know better.

The generated `.nextrs/cloudflare/` directory is disposable — gitignore it and
regenerate on demand. Vercel-provider crons deploy with the app itself; the
Worker redeploys with `nextrs cron deploy` whenever schedules change.

The worked example is `examples/react-todos`: `nextrs.toml` declares a
10-minute heartbeat, and `app/api/cron/heartbeat/route.rs` is the gated route.

See [dependencies](/docs/dependencies) for the full tooling list.
