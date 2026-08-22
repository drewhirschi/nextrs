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

A cron target is an ordinary API route, gated by `nextrs::cron::authorize`.
Both trigger providers send `Authorization: Bearer $CRON_SECRET`; the check is
fail-closed — if `CRON_SECRET` is unset, every request is rejected.

```rust
// app/api/cron/refresh/route.rs
use axum::http::{HeaderMap, StatusCode};
use axum::Json;

#[nextrs::api]
pub async fn get(headers: HeaderMap) -> Result<Json<Report>, StatusCode> {
    nextrs::cron::authorize(&headers)?;
    // ... the actual work ...
}
```

Delivery is at-least-once and imprecise, and redundant delivery from both
providers must be harmless — write handlers idempotently (compute a
deterministic time slot and tolerate redelivery rather than assuming exactly
one call per tick).

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

The generated `.nextrs/cloudflare/` directory is disposable — gitignore it and
regenerate on demand. Vercel-provider crons deploy with the app itself; the
Worker redeploys with `nextrs cron deploy` whenever schedules change.

The worked example is `examples/react-todos`: `nextrs.toml` declares a
10-minute heartbeat, and `app/api/cron/heartbeat/route.rs` is the gated route.

See [dependencies](/docs/dependencies) for the full tooling list.
