//! Demo background job — the `app/jobs/<name>/job.rs` convention.
//!
//! Calling `crate::jobs::audit_todo(payload)` from a handler does NOT run
//! this body: the `#[nextrs::job]` macro re-emits `audit_todo` as a typed
//! enqueue wrapper that persists a job row and POSTs `/__nx/jobs/audit-todo`
//! on this deployment. The body runs inside that request, behind the
//! framework-managed `WaitUntil`, with retries (`max_attempts`) and a
//! per-attempt timeout — no `WaitUntil`, HTTP, or retry code here.
//!
//! `Extension<TodosCtx>` is app state, sourced from the executing request's
//! extensions (the layer installed in main.rs / api/index.rs) — the same
//! mechanism route handlers and seed companions use.

use axum::Extension;
use react_todos::core::todos::TodosCtx;
use serde::{Deserialize, Serialize};

/// The job's payload — any `Serialize + Deserialize` type; it round-trips
/// through the job row as JSON.
#[derive(Serialize, Deserialize)]
pub struct AuditTodo {
    pub id: u64,
    pub title: String,
}

/// The job's return value — stored on the row, shown in /__nx/admin/jobs.
#[derive(Serialize)]
pub struct Audited {
    pub open_todos: usize,
}

// Retries: 3 attempts, 2s back-off doubling (2s, 4s, …) capped at 30s.
#[nextrs::job(max_attempts = 3, timeout_secs = 30, backoff_secs = 2, max_backoff_secs = 30)]
pub async fn audit_todo(
    Extension(ctx): Extension<TodosCtx>,
    payload: AuditTodo,
) -> Result<Audited, String> {
    // Real apps would write an audit log, call a webhook, sync a search
    // index… The demo proves the pieces: payload round-trip, app state, a
    // stored return value, and retries — every line below lands in the
    // attempt's log at /__nx/admin/jobs.
    let attempt = nextrs::jobs::current().map_or(1, |job| job.attempt);
    tracing::info!(id = payload.id, title = %payload.title, attempt, "auditing todo");

    // Demo hook: titles starting with "flaky:" fail their first attempt, so
    // the dashboard shows a failed attempt, the back-off, and the retry.
    if payload.title.starts_with("flaky:") && attempt == 1 {
        tracing::warn!("audit webhook answered 503");
        return Err("audit webhook 503 (demo: 'flaky:' titles fail their first attempt)".into());
    }

    let open_todos = ctx.list(true).await.map_err(|e| e.to_string())?.len();
    tracing::info!(open_todos, "audit: todo created (ran as a background job)");
    Ok(Audited { open_todos })
}
