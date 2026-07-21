//! Todos API — the adapter for `react_todos::core::todos`. Handlers stay thin:
//! extract, delegate to core, map to the wire DTOs that live here.

use axum::{Extension, Json};
use axum::extract::Query;
use react_todos::core::todos::TodosCtx;
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
// `Extension<TodosCtx>` is the demo's stand-in for a DB handle. The handler
// keeps its seed companion: the companion sources the context from the
// request extensions (installed by the layer in main.rs / api/index.rs), so
// prefetch.rs seeds through this handler unchanged.
// `Timing` records the store lookup as a `db` segment: visible in the
// response's Server-Timing header (DevTools → Network → Timing) and in the
// page's breakdown when prefetch.rs seeds through this handler.
pub async fn get(
    Extension(ctx): Extension<TodosCtx>,
    timing: nextrs::Timing,
    Query(f): Query<TodosFilter>,
) -> Json<Vec<Todo>> {
    let open_only = f.status.as_deref() == Some("open");
    let todos = timing.span("db", ctx.list(open_only)).await;
    Json(todos.into_iter().map(Into::into).collect())
}

#[nextrs::api(
    post,
    operation_id = "addTodo",
    responses((status = 200, description = "The created todo", body = Todo)),
)]
pub async fn post(
    Extension(ctx): Extension<TodosCtx>,
    wait: nextrs::WaitUntil,
    Json(req): Json<AddTodoRequest>,
) -> Json<Todo> {
    let todo: Todo = ctx.add(req.title).await.into();
    // Background work after the response: locally this is a plain spawn; on
    // Vercel (behind StreamingVercelLayer) it's registered with the runtime's
    // waitUntil so it isn't killed when the invocation ends.
    let title = todo.title.clone();
    wait.wait_until(async move {
        tracing::info!(title, "audit: todo created (ran after the response)");
    });
    Json(todo)
}
