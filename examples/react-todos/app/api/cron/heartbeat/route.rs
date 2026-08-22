//! Cron demo route. Declared in the app root's `nextrs.toml`; the generated
//! Cloudflare Worker (`nextrs cron generate` → `.nextrs/cloudflare/`) fetches
//! it on schedule with `Authorization: Bearer $CRON_SECRET`. The route itself
//! is an ordinary API route — `nextrs::cron::authorize` is the only cron-
//! specific line, and it is fail-closed when `CRON_SECRET` is unset.

use axum::extract::Extension;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use react_todos::core::todos::TodosCtx;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema)]
pub struct Heartbeat {
    pub open_todos: usize,
}

#[nextrs::api]
pub async fn get(
    headers: HeaderMap,
    Extension(ctx): Extension<TodosCtx>,
) -> Result<Json<Heartbeat>, StatusCode> {
    nextrs::cron::authorize(&headers)?;
    let open_todos = ctx.list(true).await.len();
    tracing::info!(open_todos, "cron heartbeat");
    Ok(Json(Heartbeat { open_todos }))
}
