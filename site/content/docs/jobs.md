+++
title = "Background Jobs"
description = "Tag a Rust function #[nextrs::job]: typed enqueue, retries with back-off, stored results, and every attempt's logs — on Vercel or locally"
section = "Guides"
order = 16
+++

This feature currently requires the framework and CLI from this repository's
source; it is not part of the published 0.6.1 framework / 0.3.0 CLI.

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

The call writes a row, then POSTs `/__nx/jobs/send-welcome` on your own
deployment, where the body runs behind `WaitUntil`. Because the row is
written first, a lost kick-off only delays the job.

## What happens when it fails

```text
t=0     enqueue → row {queued, payload} → run
t=0.1s  attempt 1 → Err("smtp 503") → row {failed, next_run_at: +2s}, attempt saved with its log lines
t=2.1s  attempt 2 → Ok({"message_id": "m_123"}) → row {succeeded, result}
```

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
- **Delivery is at least once.** A job whose instance dies mid-run is
  reclaimed and runs again, so make job bodies safe to repeat.

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
| `NEXTRS_JOBS_SECRET` | Required on Vercel: instances deliver jobs to each other with it. |
| `NEXTRS_JOBS_RETENTION_DAYS` | Finished rows to keep (default 30). |

On Vercel, enqueue fails loudly when the database or the secret is missing.
It never falls back to a queue that would silently lose jobs.

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
