+++
title = "Realtime"
description = "Push changes to every open tab: publish from Rust after a write, refetch in React — in-process locally, through a generated Cloudflare Durable Object relay on Vercel"
section = "Guides"
order = 18
+++

This feature currently requires the framework and CLI from this repository's
source; it is not part of the published 0.6.1 framework / 0.3.0 CLI.

Open your app in two tabs, change something in one, and the other updates
within a moment. Your Rust app makes every decision: who may listen, what a
change says, and where data comes from. The transport only fans messages
out.

## The four pieces

**1. Publish after the write.** Do it in `WaitUntil`, so the fan-out never
delays the response:

```rust
#[nextrs::api]
pub async fn post(Extension(ctx): Extension<TodosCtx>, wait: nextrs::WaitUntil, Json(req): Json<AddTodoRequest>)
    -> Result<Json<Todo>, ApiError>
{
    let todo: Todo = ctx.add(req.title).await.map_err(db_error)?.into();
    wait.wait_until(async move {
        let change = nextrs::realtime::RealtimeChange::upsert(todo.id.to_string(), json!(&todo));
        let _ = nextrs::realtime::publish("todos", change).await;
    });
    Ok(Json(todo))
}
```

**2. Decide who may listen.** Your route checks the session, then returns
a signed ticket:

```rust
// app/api/live/ticket/route.rs
#[nextrs::api]
pub async fn get(Query(q): Query<TicketQuery>) -> Result<Json<nextrs::realtime::Ticket>, ApiError> {
    if q.topic != "todos" {
        return Err(ApiError::forbidden("no access to that topic"));
    }
    nextrs::realtime::ticket(&q.topic).map(Json).map_err(|_| ApiError::internal("realtime is off"))
}
```

**3. Listen in React.** The generated client asks your ticket route before
every connect:

```tsx
const status = useLiveTopic({
  topic: "todos",
  ticket: () => getApiLiveTicket({ topic: "todos" }).then((r) => r.data),
  onChange: invalidate,   // refetch through your normal typed hooks
  onResync: invalidate,   // missed frames or a reconnect: refetch too
});
```

**4. The transport, which you don't write.** Locally and on single servers,
nextrs serves the WebSockets itself at `/__nx/realtime/{topic}`. On Vercel,
instances can't share sockets, so the CLI generates a Cloudflare relay.

## Frames

Every transport sends the same JSON. Each topic has its own sequence:

```json
{"type":"ready","topic":"todos","sequence":7}
{"type":"change","topic":"todos","sequence":8,"operation":"upsert","key":"4","value":{"id":4,"title":"milk","done":false}}
{"type":"resync","topic":"todos","sequence":12}
```

- **Operations** are `upsert`, `delete`, and `invalidate`.
- **The client refetches** on a sequence gap, a `resync`, or a reconnect,
  so it never shows stale data after missing something.
- **Simplest use:** refetch on every change, as above. Patching the cache
  from `value` is an optimization you can add later.

## Production: the Cloudflare relay

```bash
nextrs realtime generate   # .nextrs/cloudflare/realtime/{worker.js,wrangler.toml}
nextrs realtime deploy     # wrangler deploy + sets NEXTRS_REALTIME_SECRET on the worker
```

Then set the same secret, and the worker's URL, on the app:

| Env | Where | Purpose |
|---|---|---|
| `NEXTRS_REALTIME_SECRET` | app + worker | Signs tickets (HMAC-SHA256) and authenticates publishes. 16+ characters. |
| `NEXTRS_REALTIME_URL` | app | The relay, e.g. `https://my-app-realtime.you.workers.dev`. Unset means the in-process hub. |

How the relay works:
- **One Durable Object per topic** holds that topic's sockets and sequence.
- **Hibernated sockets:** idle connections cost almost nothing, so a small
  app runs comfortably on Cloudflare's free plan.
- **No app code:** the worker only checks signatures and relays. Regenerate
  it and never edit it.
- **Tickets, not cookies**, so the relay can live on its own domain.

## Enable it

```toml
nextrs = { version = "…", features = ["realtime"] }
```

`examples/react-todos` demonstrates this: open the list in two tabs, add a
todo in one, and it appears in the other.
