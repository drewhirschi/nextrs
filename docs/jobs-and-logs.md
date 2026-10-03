# Jobs, logs, and the admin dashboards

- **Date:** 2026-10-02
- **Status:** implemented on `feat/jobs-logs-admin` (2026-10-03): jobs v2 + log
  capture in ee431c4, admin portal c8dee4a, CLI d2bb7cc, react-todos demo
  2f95e54. User docs: site `/docs/jobs`, `/docs/admin`.

## As built — where the implementation differs from the design below

- **Built on jobs v1**, ported from `feat/background-jobs` (e5984ea). Jobs
  live in `app/jobs/<name>/job.rs`; enqueue is calling the function. The
  macro arguments are `max_attempts`, `timeout_secs`, `backoff_secs`, and
  `max_backoff_secs`, not `retries` / `backoff = "exponential(…)"`. The
  attempt number comes from `nextrs::jobs::current()`, not a `JobCtx`
  argument.
- **Paths:** the dashboards are `/__nx/admin/logs` and `/__nx/admin/jobs`,
  the JSON API is `/__nx/admin/api/*`, and login is `/__nx/admin/login`.
  `/__nx/jobs/*` stays v1's machine endpoints (run, sweep, status).
- **Tables** are `__nextrs_jobs` and `__nextrs_logs`.
- **Sampling** defaults to keeping everything (`NEXTRS_LOGS_SAMPLE=1.0`).
  5xx, warn, slow, and cold requests are always kept.
- **Short back-offs (≤ 60s) self-retry** inside the failed run's
  `WaitUntil`. Longer ones wait for the sweep. Locally a 30s in-process
  sweeper runs.
- **`nextrs admin set-password`** writes `NEXTRS_ADMIN_USER` and the hash to
  `.env.local` and prints the `vercel env add` lines. `nextrs deploy` does not
  push them.
- **Login lockout** is per instance, in memory, not in Turso. Argon2 is the
  main brute-force cost.
- **Not yet:** the admin routes merge into the app's router rather than a
  separate `nx-admin` server bundle. "Edit payload & retry" is not built.
  The fleet console is still later.

- **Related:** [platform-manifest.md](platform-manifest.md) (background work tiers),
  route telemetry (`crates/nextrs/src/telemetry.rs`, shipped 0.5.0),
  `WaitUntil` (`crates/nextrs/src/wait_until.rs`), server bundles
  (`site/content/docs/server-bundles.md`)

Goal: tag any function `#[nextrs::job]` and get retries, backoff, and a
dashboard showing payloads, attempts, return values, and logs, with a retry
button. Separately, keep request logs for as long as you want on any Vercel
plan (Hobby keeps 1 hour). Both features use the same log capture.

## Example 1: a job

```rust
#[nextrs::job(retries = 5, backoff = "exponential(2s, max = 10m)")]
pub async fn send_welcome(ctx: JobCtx, args: Welcome) -> anyhow::Result<EmailSent> {
    tracing::info!(user = args.user_id, "sending welcome");
    let id = resend::send(&args.email, "Welcome!").await?;   // Err → retry
    Ok(EmailSent { id })                                       // stored as the return value
}

// from any handler:
let job_id = send_welcome::enqueue(&jobs, Welcome { user_id, email }).await?;
```

What happens:

```text
t=0     enqueue → INSERT nx_jobs {status: queued, payload}, then kick a run
t=0.1s  attempt 1 → Resend 500 → attempt saved (error + logs), next_run_at = now + 2s
t=2s    attempt 2 → ok → status: succeeded, return {"id": "em_123"}, logs saved
```

- The record is written before the kick, so a lost kick only delays the job.
- The kick is an async invoke on Lambda, a signed internal POST on Vercel,
  and `tokio::spawn` locally.
- Backoff is `next_run_at`. The existing cron runs a sweeper every minute
  that picks up due and stuck jobs.
- Delivery is at-least-once, so job functions should be safe to re-run.
- Payload and return types must be serializable; the macro enforces it.
- `#[nextrs::job]` replaces the earlier `#[nextrs::task]` sketch: a job is a
  task that also keeps a record.

## Example 2: request logs

The handler is unchanged and uses plain `tracing`:

```rust
#[nextrs::api]
pub async fn post(Extension(ctx): Extension<TodosCtx>, Json(req): Json<AddTodoRequest>) -> Result<Json<Todo>, ApiError> {
    tracing::info!(title = %req.title, "adding todo");
    let todo = ctx.add(req.title).await.map_err(|e| {
        tracing::error!(error = %e, "insert failed");
        ApiError::internal()
    })?;
    Ok(Json(todo.into()))
}
```

One record per request. Everything except `lines` already exists in
`RouteTelemetry`:

```json
{
  "id": "req_9f2a", "ts": "2026-10-01T14:58:02Z",
  "method": "POST", "route": "/api/todos", "status": 500, "ms": 5012, "cold": false,
  "segments": { "mw": 0.4, "handler": 5011 },
  "lines": [
    { "t_ms": 0,    "level": "info",  "msg": "adding todo",   "fields": { "title": "milk" } },
    { "t_ms": 5011, "level": "error", "msg": "insert failed", "fields": { "error": "turso: timeout after 5s" } }
  ]
}
```

Finding it the next day:

```text
/__nx/logs   route: /api/todos   status: 5xx   since: yesterday 14:00

 14:58:02  POST  /api/todos  500  5012ms  error  ▸
 15:03:47  POST  /api/todos  500  5009ms  error  ▸
```

```bash
nextrs logs --route /api/todos --status 5xx --since "yesterday 14:00"
```

## How capture works (shared by jobs and requests)

```text
tracing::info!(...) ──▶ nextrs capture layer
                          │ finds the enclosing nextrs.route span (or job-attempt span)
                          ▼
                   record.lines.push(line)

request ends ──▶ its wait_until work ends ──▶ wait_until(flush(record)) → store
```

- **Background work stays attached.** `WaitUntil` runs each future inside
  the request's span, so its lines land in the same record, marked
  `after_response`. The record is flushed once, after that work finishes.
- **Lines outside any request or job** (startup, sweeps) go into a
  per-instance record, flushed every few seconds.
- **Stdout logging is unchanged.** Vercel's own view keeps working.
- **Tail sampling.** The keep/drop decision happens after the request, when
  the outcome is known: keep every 5xx, warning, slow request, and cold
  start, and sample fast successful requests (default ~10%). About 1KB per
  record.
- **Blind spots.** Capture can't see platform failures: init crashes,
  timeouts, OOM kills, edge errors. Those remain only in Vercel's 1-hour
  view.

## Storage: Turso

```sql
-- the viewer's "5xx on a route since yesterday" is one query
SELECT ts, method, route, status, ms, level FROM nx_logs
WHERE route = '/api/todos' AND status >= 500 AND ts >= '2026-10-01T14:00' ORDER BY ts;
```

- `nx_logs(id, ts, route, method, status, ms, level_max, record_json)` and
  `nx_jobs(id, name, status, payload, next_run_at, attempts_json, return_json, ...)`.
- Retention: a cron deletes rows older than N days (configurable).
- R2 is an optional archive for old records. It's not the primary store,
  because filtering means listing and downloading files.
- Behind a small store trait, so another backend can be added later.

## The admin dashboards: a framework-provided server bundle

```toml
# nextrs.toml
[admin]
enabled = true
```

### Login: a root username and password from the deploy env (decided 2026-10-02)

Like Coolify's root user. Credentials come from the env the app deploys
with. There's no user table and no OAuth.

```bash
nextrs admin set-password            # prompts for a username + password, writes the hash
```

```dotenv
# .env.production.local (pushed to Vercel by nextrs deploy, like CRON_SECRET)
NEXTRS_ADMIN_USER=drew
NEXTRS_ADMIN_PASSWORD_HASH=$argon2id$v=19$m=19456,t=2,p=1$...
```

```text
GET  /__nx/logs                      → no session → 302 /__nx/login
POST /__nx/login  user=drew&password=…
     → argon2 verify against NEXTRS_ADMIN_PASSWORD_HASH (constant time)
     → Set-Cookie: nx_admin=<signed>; HttpOnly; Secure; SameSite=Strict; Path=/__nx; Max-Age=43200
GET  /__nx/logs                      → 200
```

- **Fail-closed.** If `NEXTRS_ADMIN_USER` or the hash is unset, every
  `/__nx/*` admin route returns 404. This matches `CronAuth` refusing when
  `CRON_SECRET` is unset.
- **The env holds a hash, not the password.** The CLI generates it, so the
  plaintext never lives in Vercel's env. A plain `NEXTRS_ADMIN_PASSWORD` is
  accepted in dev only, with a startup warning.
- **Sessions** are a signed cookie with a 12-hour expiry. The signing key is
  derived from the password hash, so changing the password logs out every
  session. `SameSite=Strict` covers CSRF on the Retry POST.
- **Brute force:** failed attempts are counted in Turso per IP, with a short
  lockout after 10 failures. This has to live in the database because
  serverless instances don't share memory.
- **Fleet console (later):** the same scheme, with its own credentials.

`nextrs deploy` emits a second function from the same build:

```text
__nextrs_functions/default.func    ← the app (unchanged, no dashboard code)
__nextrs_functions/nx-admin.func   ← /__nx/logs, /__nx/jobs (framework code only)
```

Example: retrying a failed job.

```text
POST /__nx/jobs/job_7c11/retry ──▶ nx-admin
  └─ UPDATE nx_jobs SET status='queued', next_run_at=now WHERE id='job_7c11'
  └─ signed POST /__nx/jobs/run/job_7c11 ──▶ default function runs send_welcome
```

The admin function never runs app code. It flips rows and asks the app to
run the job, which is why it can contain only framework code.

## Later: one console for the whole fleet

Every app writes the same tables to its own Turso database, so one deployed
console can read all of them:

```text
console.hirschi.dev
  ├─ hhh          → hhh's nx_logs / nx_jobs
  ├─ finstream    → finstream's
  └─ react-todos  → react-todos'
```

Retry works the same way: flip the row, call that app's signed run
endpoint. It needs no format changes, so it can wait until 2–3 apps use the
per-app admin.

## Open questions

- Should `Edit payload & retry` exist in v1? It's convenient, but it creates
  a new job rather than mutating history.
- Per-app sampling and retention defaults.
- Sub-minute retries: self-scheduled kick vs waiting for the sweeper.

## Validation

- react-todos: a demo job that fails once and then succeeds shows two
  attempts with logs in `/__nx/jobs`, and Retry re-runs it.
- A forced 500 on `/api/todos` appears in `/__nx/logs` with its error line
  the next day (not lost after 1 hour).
- `wait_until` lines land in the originating request's record.
- The default function's binary size is unchanged with `[admin]` enabled
  (the dashboard lives only in `nx-admin`).
