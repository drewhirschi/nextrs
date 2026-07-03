//! Per-todo API. The `{id}` comes from the `[id]` directory, extracted with
//! Axum's `Path`.
//!
//! `get` declares NO `params(...)` — `#[nextrs::api]` infers it from the
//! `Path<u64>` extractor zipped with the `{id}` URL segment, so the OpenAPI
//! spec (and the generated client's types) can't drift from the signature.
//! Being a `Path`-param GET returning `Json<...>`, it also gets a typed seed
//! companion (`get_api_todos_by_id`) that `app/todos/[id]/prefetch.rs` uses.

use axum::Json;
use axum::extract::Path;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Wire shape of a single-todo read. (Named apart from the list DTO in
/// `../route.rs` — OpenAPI schema names are global.)
#[derive(Serialize, Deserialize, ToSchema)]
pub struct TodoDetail {
    pub id: u64,
    pub title: String,
    pub done: bool,
}

#[nextrs::api(
    get,
    responses((status = 200, description = "The todo, or null if unknown", body = Option<TodoDetail>)),
)]
pub async fn get(Path(id): Path<u64>) -> Json<Option<TodoDetail>> {
    Json(
        react_todos::core::todos::get(id)
            .await
            .map(|t| TodoDetail {
                id: t.id,
                title: t.title,
                done: t.done,
            }),
    )
}

/// Body for `PATCH /api/todos/{id}`.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct UpdateTodoRequest {
    pub done: bool,
}

// The `{id}` path param is inferred from the signature; the body is declared
// like any utoipa request_body.
#[nextrs::api(
    patch,
    operation_id = "updateTodo",
    request_body = UpdateTodoRequest,
    responses((status = 200, description = "The updated todo, or null if unknown", body = Option<TodoDetail>)),
)]
pub async fn patch(
    Path(id): Path<u64>,
    Json(req): Json<UpdateTodoRequest>,
) -> Json<Option<TodoDetail>> {
    Json(
        react_todos::core::todos::set_done(id, req.done)
            .await
            .map(|t| TodoDetail {
                id: t.id,
                title: t.title,
                done: t.done,
            }),
    )
}

#[nextrs::api(
    delete,
    operation_id = "deleteTodo",
    params(("id" = u64, Path, description = "Id of the todo to delete")),
    responses(
        (status = 200, description = "Deleted"),
        (status = 404, description = "No todo with that id"),
    ),
)]
pub async fn delete(Path(id): Path<u64>) -> StatusCode {
    if react_todos::core::todos::remove(id).await {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}
