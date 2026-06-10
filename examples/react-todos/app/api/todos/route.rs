//! Todos API — the adapter for `react_todos::core::todos`. Handlers stay thin:
//! extract, delegate to core, map to the wire DTOs that live here.

use axum::Json;
use axum::extract::Query;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// Wire shape of a todo. Owned by this adapter; `From` maps the core type.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct Todo {
    pub id: u64,
    pub title: String,
    pub done: bool,
}

impl From<react_todos::core::todos::Todo> for Todo {
    fn from(t: react_todos::core::todos::Todo) -> Self {
        Self {
            id: t.id,
            title: t.title,
            done: t.done,
        }
    }
}

/// Filter for `GET /api/todos`.
///
/// `skip_serializing_if` matters beyond cosmetics: seeded query keys are
/// hashed client-side where absent fields are dropped — serializing `None`
/// as `null` would make the Rust-built key never match the hook's.
#[derive(Serialize, Deserialize, IntoParams)]
pub struct TodosFilter {
    /// `"open"` returns only unfinished todos; anything else returns all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// Request body for `POST /api/todos`.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct AddTodoRequest {
    pub title: String,
}

#[nextrs::api(
    get,
    operation_id = "getTodos",
    params(TodosFilter),
    responses((status = 200, description = "List todos", body = Vec<Todo>)),
)]
pub async fn get(Query(f): Query<TodosFilter>) -> Json<Vec<Todo>> {
    let open_only = f.status.as_deref() == Some("open");
    Json(
        react_todos::core::todos::list(open_only)
            .await
            .into_iter()
            .map(Into::into)
            .collect(),
    )
}

#[nextrs::api(
    post,
    operation_id = "addTodo",
    responses((status = 200, description = "The created todo", body = Todo)),
)]
pub async fn post(Json(req): Json<AddTodoRequest>) -> Json<Todo> {
    Json(react_todos::core::todos::add(req.title).await.into())
}
