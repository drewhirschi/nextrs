//! Server prefetch for the todo detail page. The route is dynamic
//! (`/todos/[id]`), so the generated handler calls `prefetch(req, params)` —
//! the matched params arrive typed as [`nextrs::Params`], no URI parsing.
//!
//! `get_api_todos_by_id` is the seed companion for the FALLIBLE path+query
//! handler `GET /api/todos/{id}?neighbors=`: it takes the typed id and query
//! struct, substitutes into the URL, and keys the entry exactly like the
//! generated `useGetApiTodosByIdFromUrl(id)` hook keys the same request —
//! query parsed from the SAME search string the client hook reads, so
//! `/todos/2?neighbors=true` seeds `["/api/todos/2", {"neighbors": true}]`.
//! An unknown id makes the handler return Err: the companion seeds nothing
//! and the page's fetch surfaces the 404 client-side.

include!(concat!(env!("OUT_DIR"), "/nextrs_seeds.rs"));

pub async fn prefetch(
    req: http::Request<axum::body::Body>,
    params: nextrs::Params,
) -> nextrs::QuerySeed {
    let id: u64 = params
        .get("id")
        .and_then(|v| v.parse().ok())
        .unwrap_or_default();
    let query = nextrs::search_params::<api_todos_by_id::DetailQuery, _>(&req)
        .unwrap_or(api_todos_by_id::DetailQuery { neighbors: None });
    nextrs::QuerySeed::new()
        .seed(get_api_todos_by_id(id, query, req.extensions()))
        .await
}
