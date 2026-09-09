use axum::{
    extract::{Path, Query},
    http::HeaderMap,
    response::IntoResponse,
};
use std::collections::HashMap;

static HANDLER_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub async fn post(
    axum::Extension((_, chain)): axum::Extension<(bool, Vec<&'static str>)>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    (
        http::StatusCode::CREATED,
        [
            ("x-id", id),
            ("x-middleware-chain", chain.join(",")),
            ("x-handler-calls", (HANDLER_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1).to_string()),
            ("x-query", query.get("q").cloned().unwrap_or_default()),
            (
                "x-cookie",
                headers.get("cookie").unwrap().to_str().unwrap().to_string(),
            ),
            (
                "x-guard-ran",
                headers
                    .get("x-guard-ran")
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string(),
            ),
            ("set-cookie", "result=ok; Path=/; HttpOnly".into()),
        ],
        body,
    )
}

pub async fn get() -> impl IntoResponse {
    let stream = async_stream::stream! {
        yield Ok::<_, std::io::Error>("first\n");
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        yield Ok::<_, std::io::Error>("second\n");
    };
    axum::body::Body::from_stream(stream)
}
