# waitUntil — background work after the response

- **Reported-in:** finstream
- **Date:** 2026-07-18
- **Status:** fixed in 2ff2c63 (demo/docs in 91807e7)

## Problem

Handlers have no sanctioned way to run work after the response is sent
(audit logs, analytics, cache writes, webhooks). On Vercel, `tokio::spawn`
is not enough: the runtime may freeze or terminate the instance once the
invocation ends, so detached tasks silently die. `vercel_runtime` 2.4
ships the fix — `AppState::wait_until` registers futures with a
process-global `Awaiter` drained at SIGTERM — but nextrs's
`StreamingVercelService` discards the `AppState` (`(_state, req)`), so
apps can't reach it.

## Proposed Direction

A portable `nextrs::WaitUntil` axum extractor:

- Default (local dev, Docker, self-hosted long-lived servers): plain
  `tokio::spawn` — the process outlives the request anyway.
- On Vercel: `StreamingVercelService` inserts a `WaitUntil` backed by
  `AppState::wait_until` into the request extensions before calling the
  router, so the same handler code gets shutdown-drained semantics with
  zero app changes.

Handlers write:

```rust
pub async fn post(wait: nextrs::WaitUntil, Json(req): Json<...>) -> ... {
    wait.wait_until(async move { /* audit log, webhook, ... */ });
    ...
}
```

Framework-side wiring means existing apps adopt it on `cargo update` —
no scaffold/template change needed (the injection lives in the layer,
not the generated `main.rs`).

## Implementation Notes

- `crates/nextrs/src/wait_until.rs` — `WaitUntil` (Clone, Default) holding
  an optional boxed scheduler; `FromRequestParts` impl is infallible and
  falls back to the spawn-backed default when no extension is present.
- `crates/nextrs/src/vercel.rs` — stop discarding the `AppState`; insert
  `WaitUntil::from_scheduler(move |fut| state.wait_until(fut))`.
- Seed companions: an extra extractor disqualifies a GET handler from
  seeding (existing behavior, unchanged). The typical waitUntil consumer
  is a mutation handler, so this doesn't bite; if a seeded GET ever needs
  it, teach the macro to pass `WaitUntil::detached()` — follow-up.

## Validation

- Unit test: extractor with no extension → spawn fallback runs the future.
- Unit test: `StreamingVercelService` injects a scheduler that forwards to
  `AppState::wait_until` (observed via a channel).
- react-todos POST /api/todos registers background work; run the example
  and confirm the log line lands after the response.
