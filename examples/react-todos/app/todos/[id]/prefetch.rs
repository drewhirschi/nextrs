//! Server prefetch for the todo detail page. The route is dynamic
//! (`/todos/[id]`), so the generated handler calls `prefetch(req, params)` —
//! the matched params arrive typed as [`nextrs::Params`], no URI parsing.
//!
//! `get_api_todos_by_id` is the Path-param seed companion `#[nextrs::api]`
//! emits for `GET /api/todos/{id}`: it takes the typed id, substitutes it
//! into the URL, and keys the entry `["/api/todos/7"]` — exactly how the
//! generated `useGetApiTodosById(id)` hook keys the same request.

include!(concat!(env!("OUT_DIR"), "/nextrs_seeds.rs"));

pub async fn prefetch(
    req: http::Request<axum::body::Body>,
    params: nextrs::Params,
) -> nextrs::QuerySeed {
    let id: u64 = params
        .get("id")
        .and_then(|v| v.parse().ok())
        .unwrap_or_default();
    nextrs::QuerySeed::new()
        .seed(get_api_todos_by_id(id, req.extensions()))
        .await
}
