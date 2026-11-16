+++
title = "Logs and Admin"
description = "Keep request logs for days on any Vercel plan, and read them — plus your jobs — in a built-in admin portal behind a root login"
section = "Guides"
order = 17
+++

This feature currently requires the framework and CLI from this repository's
source; it is not part of the published 0.6.1 framework / 0.3.0 CLI.

Vercel Hobby keeps runtime logs for **one hour**. nextrs can save each
request's logs to a database you own, and serve them, with your
[jobs](/docs/jobs), at `/__nx/admin`.

## What you get

A user says adding a todo failed yesterday afternoon. With logs saved:

```text
/__nx/admin/logs   status: 5xx   since: last 24h

 10/2 14:58:02  POST  /api/todos  500  5012  error  ▸
```

Click the row:

```text
POST /api/todos 500 · 5012 ms

 mw      ▏0.4 ms
 handler ████████████████████ 5011 ms

 +0ms     info   adding todo     title=milk
 +5011ms  error  insert failed   error=turso: timeout after 5s
```

You write nothing new. Your handlers keep using `tracing`:

```rust
#[nextrs::api]
pub async fn post(Extension(ctx): Extension<TodosCtx>, Json(req): Json<AddTodoRequest>) -> Result<Json<Todo>, ApiError> {
    tracing::info!(title = %req.title, "adding todo");
    let todo = ctx.add(req.title).await.map_err(|e| {
        tracing::error!(error = %e, "insert failed");
        ApiError::internal("could not add the todo")
    })?;
    Ok(Json(todo.into()))
}
```

## How lines are captured

- **One record per request:** route, status, duration, cold start, timing
  segments (including your [`Timing`](/docs/telemetry) spans), and every
  line logged while it ran.
- **Background work stays with its request.** Lines from
  `wait.wait_until(...)` work land in the same record, marked "after
  response". The record is saved once that work finishes.
- **The same capture records each job attempt's lines.**
- **Tail sampling:** every 5xx, warning, slow (> 500 ms), and cold-start
  request is kept. `NEXTRS_LOGS_SAMPLE=0.1` keeps 10% of the rest; the
  default keeps everything.
- **What it can't see:** anything that kills the process before the record
  is saved (init panics, timeouts, out-of-memory) and platform or edge
  errors. Those stay in Vercel's own log view.

## Enable it

```toml
# Cargo.toml — `admin` includes `jobs` and `logs`
nextrs = { version = "…", features = ["vercel", "admin", "libsql"] }
```

Install the capture layer next to your formatter, in each entry point
(`src/main.rs`, `api/index.rs`):

```rust
use tracing_subscriber::prelude::*;

tracing_subscriber::registry()
    .with(tracing_subscriber::EnvFilter::new("info"))
    .with(tracing_subscriber::fmt::layer())
    .with(nextrs::logs::layer())
    .init();
```

| Env | Purpose |
|---|---|
| `NEXTRS_DB_URL` / `TURSO_DATABASE_URL` (+ `_TOKEN`) | Where records live (`__nextrs_logs`). Memory when unset, which is fine for dev. |
| `NEXTRS_LOGS_RETENTION_DAYS` | Days to keep (default 14), enforced by the jobs sweep. |
| `NEXTRS_LOGS_SAMPLE` | Sample rate for fast, successful, quiet requests (default 1.0). |
| `NEXTRS_LOGS=0` | Turn capture off. |

## The root login

Like Coolify's root user, the admin credentials come from your deploy
environment. There is no user table:

```bash
nextrs admin set-password
# Username [drew]:
# Password: ********
# wrote NEXTRS_ADMIN_USER and NEXTRS_ADMIN_PASSWORD_HASH to .env.local
```

Add both variables to the deployment (`vercel env add …`). Only the argon2
hash goes there, never the password.

- **Fail-closed:** with either variable unset, every `/__nx/admin` route
  answers 404.
- **Sessions** are a signed, `HttpOnly`, `SameSite=Strict` cookie that
  lasts 12 hours. Changing the password logs every session out.
- **Lockout:** a client is locked out for 15 minutes after 10 failed
  logins. Clients are told apart by `X-Forwarded-For` only behind a proxy
  that sets it: Vercel, or `NEXTRS_TRUST_PROXY=1`. Otherwise every direct
  client shares one bucket.
- **Dev shortcut:** a plain `NEXTRS_ADMIN_PASSWORD` works locally, never on
  Vercel.

## From the terminal

The same data, as the same user, from `app.url` in `nextrs.toml` (or
`--url`):

```bash
nextrs logs --status 5xx --since 24h
nextrs logs req_7c4eff806ce6ecd4     # one request's timeline
nextrs jobs --status failed
nextrs jobs retry <id>
```

The password comes from `NEXTRS_ADMIN_PASSWORD` or a prompt. Scripts can call
the JSON API under `/__nx/admin/api/` with HTTP Basic.

`examples/react-todos` demonstrates this: add a todo titled `boom`, then
open `/__nx/admin/logs`.
