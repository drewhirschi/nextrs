//! Server prefetch for the todos page. The filter is URL state (`?status=open`
//! — see the page's `useGetTodosFromUrl`), so the seed derives from the SAME
//! query string the client hook reads: `nextrs::search_params` parses it into
//! the handler's `TodosFilter`, and the typed companion keys the entry exactly
//! like the hook keys the request. A shared filtered link therefore renders
//! seeded — no fetch on first paint, for any filter.
//!
//! `get_api_todos` is the companion `#[nextrs::api]` emits for the GET handler
//! in `app/api/todos/route.rs`; it calls the real handler (the wire contract).

include!(concat!(env!("OUT_DIR"), "/nextrs_seeds.rs"));

pub async fn prefetch(req: http::Request<axum::body::Body>) -> nextrs::QuerySeed {
    let filter = nextrs::search_params::<api_todos::TodosFilter, _>(&req)
        .unwrap_or(api_todos::TodosFilter { status: None });
    nextrs::QuerySeed::new()
        .seed(get_api_todos(filter, req.extensions()))
        .await
}
