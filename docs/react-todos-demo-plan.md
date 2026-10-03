# react-todos: demonstrating jobs, logs, admin, and realtime

- **Date:** 2026-10-02
- **Status:** plan, nothing built
- **Designs:** [jobs-and-logs.md](jobs-and-logs.md), realtime relay (below; no
  standalone doc yet), [platform-manifest.md](platform-manifest.md)

`examples/react-todos` is the living reference: every framework feature ships
with a demo there, in the same PR. This plan covers what each demo looks like
from the browser, which files change, and the order to build them in. Each
step is one PR. Each is verified by running the app
(`cargo build -p react-todos && PORT=<free> ./target/debug/react-todos`) and
by building `site/` with bundling on.

## Step 0: a real store (prerequisite)

**Why:** todos currently live in `Mutex<Vec<Todo>>` (`src/core/todos.rs`).
On Vercel every instance has its own list:

```text
Tab A adds "milk" → instance 1's list has milk
Tab B refetches   → instance 2's list doesn't → realtime looks broken
```

Jobs and logs need persistence too.

**Change:** `TodosCtx` wraps a libsql connection: a local `todos.db` file in
dev, Turso in production (`TURSO_URL` / `TURSO_TOKEN`). The handlers don't
change, because `ctx.add` / `ctx.list` keep their signatures. Startup creates
the table and seeds the same three demo todos.

**Demo:** add a todo in production, hard-reload a few times, and it's still
there.

## Step 1: request logs

**Demo:**

```text
1. Add a todo titled "boom"          → the API returns 500 (simulated insert failure)
2. Open /__nx/logs                   → the row is at the top
3. Click it                          → the error line + waterfall
4. Come back tomorrow                → still there (Vercel's own view would have lost it)
```

```text
 14:58:02  POST  /api/todos  500  12ms  error  ▸

 +0ms   info   adding todo    title="boom"
 +11ms  error  insert failed  error="simulated failure (demo: title 'boom')"
```

**Files:**
- `src/core/todos.rs`: `add` returns `Err` for the title `"boom"`. A
  clearly marked demo hook.
- `app/api/todos/route.rs`: `post` returns `Result<Json<Todo>, ApiError>`
  with `tracing::error!` on failure (the code in jobs-and-logs.md, Example 2).
- `nextrs.toml`: `[logs] store = "turso"`, `retention_days = 14`.
- The existing `wait_until` audit line in `post` shows up in the same record,
  marked `after_response`. That's a free demo of background lines staying
  attached.

## Step 2: a job

**Demo:**

```text
1. Add a todo titled "flaky: call mom"
2. Open /__nx/jobs                   → announce_todo  ● running
3. ~2s later                         → ✓ succeeded, 2 attempts
4. Click it → attempt 1 ✗ "webhook 503 (demo: flaky)", attempt 2 ✓ return {"delivered_at": ...}
5. Press [Retry]                     → a 3rd attempt runs and succeeds
```

**Files:**
- `app/jobs.rs` (new). It holds the job; any non-route module in `app/` is
  allowed:

  ```rust
  #[nextrs::job(retries = 3, backoff = "exponential(2s, max = 1m)")]
  pub async fn announce_todo(ctx: JobCtx, args: Announce) -> anyhow::Result<Announced> {
      tracing::info!(id = args.todo_id, "announcing");
      if args.title.starts_with("flaky:") && ctx.attempt() == 1 {
          anyhow::bail!("webhook 503 (demo: flaky)");
      }
      Ok(Announced { delivered_at: now() })
  }
  ```

- `app/api/todos/route.rs`: `post` calls
  `announce_todo::enqueue(&jobs, ...)` after the insert.
- `app/api/cron/heartbeat/route.rs`: the existing cron also runs the job
  sweeper (`jobs.sweep()`), which is what makes retries happen on Vercel.

## Step 3: the admin function

**Demo:**

```text
1. nextrs admin set-password         → writes NEXTRS_ADMIN_USER + NEXTRS_ADMIN_PASSWORD_HASH
2. nextrs bundles plan               → lists default + nx-admin
3. Visit /__nx/logs while logged out → /__nx/login
4. Root username + password          → logs and jobs dashboards (steps 1–2)
5. Unset the env vars, redeploy      → /__nx/* returns 404 (fail-closed)
```

**Files:**
- `nextrs.toml`: `[admin] enabled = true`.
- `.env.production.local`: the two admin vars. `nextrs deploy` pushes them
  to Vercel, as it does `CRON_SECRET`.
- Nothing else. The dashboards and login are framework code, which is the
  point of this demo.

**Check:** the default function's binary size doesn't change when `[admin]`
is turned on.

## Step 4: realtime

**Demo:**

```text
1. Open the todo list in two tabs side by side
2. Add "milk" in tab A               → appears in tab B within ~1s, no reload
3. Toggle it done in tab B           → tab A updates
4. In nextrs dev: same behavior, no Cloudflare (in-process hub)
```

**Files:**
- `app/api/todos/route.rs` and `app/api/todos/[id]/route.rs`: after each
  mutation, `wait.wait_until(live.publish("todos"))`.
- `app/api/live/token/route.rs` (new): signs subscribe tokens for a topic.
- `app/page.tsx`: one line next to the existing `invalidate`:

  ```tsx
  useLiveTopic("todos", invalidate);
  ```

- `nextrs.toml`: `[live] provider = "cloudflare"` (prod) / in-process hub
  (dev).
- Generated, never edited: `.nextrs/cloudflare/live/worker.js` +
  `wrangler.toml` (the relay Durable Object, with hibernation and
  auto-response pings). Deploy with `nextrs live deploy`, or as part of
  `nextrs deploy` like the cron shim.

## Not in this plan

- **The Lambda target and the artifact cache** are deploy-level. When the
  platform exists, react-todos is a natural first dogfood target alongside
  hhh, using this same app.
- **The fleet console** comes later, once 2–3 apps write `nx_logs` /
  `nx_jobs`.

## Order and why

| Step | Builds | Depends on |
|---|---|---|
| 0 store | libsql `TodosCtx` | — |
| 1 logs | capture layer, `nx_logs`, `/__nx/logs` | 0 |
| 2 jobs | `#[nextrs::job]`, `nx_jobs`, sweeper, `/__nx/jobs` | 0, reuses 1's capture layer |
| 3 admin | `nx-admin` server bundle + login | 1, 2 (gives it something to show) |
| 4 realtime | `nextrs::Live`, relay generator, `useLiveTopic` | 0 (tabs must share data) |

Step 4 only depends on step 0, so it could move earlier if realtime becomes
more pressing than jobs.

## Open questions

- **Step 0 grows the example.** CLAUDE.md wants react-todos kept small, but
  a real DB is what every real app has, and three of the four demos need it.
  Keep the libsql code to one file.
- **Production credentials** for the react-todos Vercel project: a Turso DB
  and a Cloudflare account (the cron shim already uses one).
- **The "boom" / "flaky:" demo hooks** live in app code, so they're visible
  to readers. That's fine for a demo, but they should be clearly labeled.
