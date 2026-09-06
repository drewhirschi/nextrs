use axum::{
    extract::{Path, Query},
    http::HeaderMap,
    response::IntoResponse,
};
use std::collections::HashMap;

pub async fn post(
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    (
        http::StatusCode::CREATED,
        [
            ("x-id", id),
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
