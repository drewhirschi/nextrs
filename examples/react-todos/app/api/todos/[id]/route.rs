//! Per-todo API. `DELETE /api/todos/{id}` — the `{id}` comes from the
//! `[id]` directory, extracted with Axum's `Path`.

use axum::extract::Path;
use axum::http::StatusCode;

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
