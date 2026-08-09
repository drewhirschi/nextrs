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

#[nextrs::job(max_attempts = 3, timeout_secs = 30)]
pub async fn audit_todo(
    Extension(ctx): Extension<TodosCtx>,
    payload: AuditTodo,
) -> Result<(), String> {
    // Real apps would write an audit log, call a webhook, sync a search
    // index… The demo proves the pieces: payload round-trip, app state, and
    // the structured job lifecycle logs around this line.
    let open = ctx.list(true).await.len();
    tracing::info!(
        id = payload.id,
        title = %payload.title,
        open_todos = open,
        "audit: todo created (ran as a background job)"
    );
    Ok(())
}
