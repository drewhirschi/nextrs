+++
title = "Background Jobs"
description = "Tag a Rust function #[nextrs::job]: typed enqueue, retries with back-off, stored results, and every attempt's logs — on Vercel or locally"
section = "Guides"
order = 16
+++

Available since nextrs 0.7.0 and the 0.3.1 CLI. Delivery through Vercel
Queues is in the release after 0.7.0.

Some work must not be lost when it fails: sending an email, syncing a
webhook, writing an audit record. `nextrs::WaitUntil` runs work after the
response, but once. A **job** is that work with a row in your database, a
retry policy, and a record of every attempt you can read in
[the admin portal](/docs/admin).

## Write a job

A job lives in `app/jobs/<name>/job.rs`. The directory is its name:

```rust
// app/jobs/send-welcome/job.rs
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Welcome {
    pub user_id: u64,
    pub email: String,
}

#[derive(Serialize)]
pub struct Sent {
    pub message_id: String,
}

// All optional. Defaults: 5 attempts, 60s timeout, 30s back-off doubling to 1h.
#[nextrs::job(max_attempts = 3, timeout_secs = 30, backoff_secs = 2, max_backoff_secs = 60)]
pub async fn send_welcome(payload: Welcome) -> Result<Sent, String> {
    tracing::info!(user = payload.user_id, "sending welcome");
    let message_id = mailer::send(&payload.email, "Welcome!").await.map_err(|e| e.to_string())?;
    Ok(Sent { message_id })
}
```

- The payload is any `Serialize + Deserialize` type; the return value any
  `Serialize` type (stored as the job's `result`), or `()`.
- `Err`, a panic, or the timeout counts as a failed attempt.
- `Extension<T>` arguments receive your app state, the same as in route
  handlers.
- `nextrs::jobs::current()` tells the body which attempt is running.

## Enqueue it

Calling the function by name enqueues it. The body doesn't run in your
handler:

```rust
let handle = crate::jobs::send_welcome(Welcome { user_id, email }).await?;
// handle.id — the job's id; the work runs on its own, after this returns
```

The call writes a row, then hands the row's id to a runner. Because the row
is written first, a lost hand-off only delays the job. How the hand-off
happens depends on where the app runs; your job code is the same either way.

## What happens when it fails

```text
t=0     enqueue → row {queued, payload} → run
t=0.1s  attempt 1 → Err("smtp 503") → row {failed, next_run_at: +2s}, attempt saved with its log lines
t=2.1s  attempt 2 → Ok({"message_id": "m_123"}) → row {succeeded, result}
```

Delivery is at least once. A job whose instance dies mid-run is reclaimed
and runs again, so make job bodies safe to repeat.

## How jobs are delivered

### On Vercel: Vercel Queues

A generated app has a second function, `api/jobs.rs`, next to
`api/index.rs`. It is the queue consumer:

```rust
// api/jobs.rs — do not put application logic here
use nextrs::vercel::StreamingVercelLayer;
use tower::ServiceBuilder;

#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    let consumer = nextrs::jobs::vercel_queues::consumer(my_app::app());
    let app = ServiceBuilder::new()
        .layer(StreamingVercelLayer::new())
        .service(consumer);

    vercel_runtime::run(app).await
}
```

```text
enqueue   → row {queued}, message {id, name} sent to the `nx-jobs` topic
delivery  → Vercel calls api/jobs → claim the row → run the job inside that request
success   → row {succeeded} → acknowledge the message
failure   → row {failed} → send a new message delayed by the back-off → acknowledge
crash     → nothing acknowledged → the queue delivers the message again
            → the row is still `running` past its timeout → reclaimed and run again
```

- **Back-off is exact.** A retry is a message delayed by
  `backoff_secs · 2^(n-1)` (up to Vercel's limit of 7 days), on any plan.
  There is no sweep to wait for.
- **No secret and no cron.** A consumer function can't be called from
  outside the deployment, so `NEXTRS_JOBS_SECRET` isn't needed, and nothing
  has to call `/__nx/jobs/sweep`.
- **The row is still the record.** The message carries only the job's id and
  name. Status, attempts, results, the admin pages, and Retry work the same.
- **A redelivery never starts a second copy of a live run.** While a job is
  still inside its timeout, a redelivered message is left alone.
- **Old rows** are deleted by the consumer, once per instance, after
  `NEXTRS_JOBS_RETENTION_DAYS`.
- **If a message can't be sent** (the queue is unreachable), the call still
  returns `Ok` with `delivered: false` and the row stays `queued`. Retry it
  from the admin page, or keep a cron on `/__nx/jobs/sweep` as a backstop:
  in this mode the sweep re-sends due rows to the queue.

`nextrs generate` writes the function's trigger into `.nextrs/vercel.json`
because `api/jobs.rs` exists, and `nextrs deploy` tells every function in
the deployment that a consumer is there. That marker is what turns queue
delivery on, so deploy with `nextrs deploy` (or the generated
`scripts/deploy-prebuilt.sh`, which does the same).

To add it to an app created before this: copy `api/jobs.rs` from above, add
the Cargo target, and deploy.

```toml
# Cargo.toml
[[bin]]
name = "jobs"
path = "api/jobs.rs"
```

A job's `timeout_secs` must fit inside the function's maximum duration,
because the job runs inside the delivery request.

Vercel Queues is in beta. The trigger type is `queue/v2beta`.

### Everywhere else: a POST to your own app

Locally, on other hosts, and on Vercel apps without `api/jobs.rs`, the call
POSTs `/__nx/jobs/send-welcome` on your own deployment, where the body runs
behind `WaitUntil`.

- **Short back-offs (≤ 60s):** the failed run sleeps and re-delivers
  itself, so `backoff_secs = 2` really means two seconds.
- **Longer back-offs:** picked up by the sweep, `GET /__nx/jobs/sweep`.
  Point a cron at it. It accepts `Authorization: Bearer $CRON_SECRET`, so a
  [`#[nextrs::cron]`](/docs/crons) schedule works as-is.
- **Locally:** with the in-memory store, a 30-second in-process sweeper does
  the same, so jobs behave the way they do in production. It stays off when
  a database is configured, so a dev server pointed at production can't run
  production's jobs. `NEXTRS_JOBS_LOCAL_SWEEP=1` turns it on, for example on
  a self-hosted server with its own database.

`NEXTRS_JOBS_DELIVERY=http` or `=vercel-queues` overrides the choice.

## Enable it

```toml
# Cargo.toml
nextrs = { version = "…", features = ["jobs", "libsql"] }
```

```rust
// src/main.rs, after binding — tells jobs where this server listens
nextrs::jobs::announce_local_addr(listener.local_addr().unwrap());
```

| Env | Purpose |
|---|---|
| `NEXTRS_DB_URL` / `TURSO_DATABASE_URL` (+ `_TOKEN`) | Where job rows live (`__nextrs_jobs`, created automatically). Memory when unset, which is fine for dev. |
| `NEXTRS_JOBS_SECRET` | Required on Vercel only without `api/jobs.rs`: instances deliver jobs to each other with it. |
| `NEXTRS_JOBS_RETENTION_DAYS` | Finished rows to keep (default 30). |
| `NEXTRS_JOBS_DELIVERY` | `http` or `vercel-queues`. Unset: Vercel Queues on Vercel when the deployment has the consumer function, otherwise HTTP. |

On Vercel, enqueue fails loudly when the database is missing, or the secret
when delivery is by POST. It never falls back to something that would
silently lose jobs.

## See and retry jobs

[`/__nx/admin/jobs`](/docs/admin) lists jobs by status. Each job page shows
the payload, the result, and every attempt's error and log lines, with a
**Retry** button. From the terminal:

```bash
nextrs jobs --status failed
nextrs jobs <id>
nextrs jobs retry <id>
```

`examples/react-todos` demonstrates this: add a todo titled `flaky: call
mom`, then open `/__nx/admin/jobs`.
