//! Server props for the todos page: pre-run the open-todos query and stream
//! it into the page, so the React Query cache is warm before the bundle
//! executes — no fetch on first paint.
//!
//! `get_api_todos` is the typed companion `#[nextrs::api]` emits for the
//! GET handler in `app/api/todos/route.rs`; it calls the real handler (the
//! wire contract) and pairs the result with the canonical query key
//! `["/api/todos", {"status":"open"}]` — the same key the generated
//! `useGetTodos({status:"open"})` hook uses, so mutations and invalidation
//! reach this data like any fetched data.

include!(concat!(env!("OUT_DIR"), "/nextrs_seeds.rs"));

pub async fn props(req: http::Request<axum::body::Body>) -> nextrs::QuerySeed {
    nextrs::QuerySeed::new()
        .seed(get_api_todos(
            api_todos::TodosFilter {
                status: Some("open".to_string()),
            },
            req.extensions(),
        ))
        .await
}
