//! Cron demo route. Its schedule is declared on `#[nextrs::cron]`; the generated
//! Cloudflare Worker (`nextrs cron generate` → `.nextrs/cloudflare/`) fetches
//! it on schedule with `Authorization: Bearer $CRON_SECRET`. The route itself
//! is an ordinary API route — `#[nextrs::cron]` is `#[nextrs::api]` plus the
//! `CronAuth` gate, which is fail-closed when `CRON_SECRET` is unset.

use axum::extract::Extension;
use axum::http::StatusCode;
use axum::Json;
use react_todos::core::todos::TodosCtx;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema)]
pub struct Heartbeat {
    pub open_todos: usize,
}

#[nextrs::cron(schedule = "*/10 * * * *", provider = "cloudflare")]
pub async fn get(Extension(ctx): Extension<TodosCtx>) -> Result<Json<Heartbeat>, StatusCode> {
    let open_todos = ctx.list(true).await.len();
    tracing::info!(open_todos, "cron heartbeat");
    Ok(Json(Heartbeat { open_todos }))
}
