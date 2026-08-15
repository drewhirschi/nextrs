//! Per-todo API. The `{id}` comes from the `[id]` directory, extracted with
//! Axum's `Path`.
//!
//! `get` declares NO `params(...)` — `#[nextrs::api]` infers it from the
//! `Path<u64>` extractor zipped with the `{id}` URL segment, so the OpenAPI
//! spec (and the generated client's types) can't drift from the signature.
//! Being a `Path`-param GET returning `Json<...>`, it also gets a typed seed
//! companion (`get_api_todos_by_id`) that `app/todos/[id]/prefetch.rs` uses.

use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::{Extension, Json};
use nextrs::ApiError;
use react_todos::core::todos::TodosCtx;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// Wire shape of a single-todo read. (Named apart from the list DTO in
/// `../route.rs` — OpenAPI schema names are global.)
#[derive(Serialize, Deserialize, ToSchema)]
pub struct TodoDetail {
    pub id: u64,
    pub title: String,
    pub done: bool,
    /// Adjacent todo ids, present when requested with `?neighbors=true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<u64>,
}

/// Options for `GET /api/todos/{id}` — a path+query route, so its generated
/// `useGetApiTodosByIdFromUrl(id)` takes the path value as an argument and
/// binds only these to the page URL. (`skip_serializing_if` keeps seeded
/// query keys matching the client's, which drops absent fields.)
#[derive(Serialize, Deserialize, IntoParams)]
pub struct DetailQuery {
    /// Include prev/next ids for detail-page navigation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub neighbors: Option<bool>,
}

// Fallible, like real handlers: `Result<Json<T>, ApiError>` is the framework's
// recommended shape. The macro infers the 200 from `Json<TodoDetail>` AND a
// `default` error response with the `ApiError` schema — no `responses(...)`
// block — so the generated client's error side is typed too. (It also gets a
// seed companion: an Err seeds nothing and the client fetches normally.)
#[nextrs::api]
pub async fn get(
    Extension(ctx): Extension<TodosCtx>,
    Path(id): Path<u64>,
    Query(q): Query<DetailQuery>,
) -> Result<Json<TodoDetail>, ApiError> {
    let todo = ctx
        .get(id)
        .await
        .ok_or_else(|| ApiError::not_found("no todo with that id").with_code("todo_not_found"))?;
    let (prev, next) = if q.neighbors.unwrap_or(false) {
        ctx.neighbors(id).await
    } else {
        (None, None)
    };
    Ok(Json(TodoDetail {
        id: todo.id,
        title: todo.title,
        done: todo.done,
        prev,
        next,
    }))
}

/// Body for `PATCH /api/todos/{id}`.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct UpdateTodoRequest {
    pub done: bool,
}

// Also fully inferred: the `{id}` path param from `Path<u64>`, the request
// body from `Json<UpdateTodoRequest>`, the nullable 200 from
// `Json<Option<TodoDetail>>`.
#[nextrs::api]
pub async fn patch(
    Extension(ctx): Extension<TodosCtx>,
    Path(id): Path<u64>,
    Json(req): Json<UpdateTodoRequest>,
) -> Json<Option<TodoDetail>> {
    Json(
        ctx.set_done(id, req.done)
            .await
            .map(|t| TodoDetail {
                id: t.id,
                title: t.title,
                done: t.done,
                prev: None,
                next: None,
            }),
    )
}

// Effect-only endpoint: a bare `StatusCode` return infers a body-less 200 —
// the escape hatch for handlers with nothing to serialize.
#[nextrs::api]
pub async fn delete(Extension(ctx): Extension<TodosCtx>, Path(id): Path<u64>) -> StatusCode {
    if ctx.remove(id).await {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}
