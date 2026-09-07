pub async fn handle(
    mut req: http::Request<axum::body::Body>,
) -> nextrs::conventions::MiddlewareResult {
    let admin = match req.headers().get("authorization").and_then(|v| v.to_str().ok()) {
        Some("Bearer test") => true,
        Some("Bearer member") => false,
        _ => return nextrs::conventions::MiddlewareResult::response(http::StatusCode::UNAUTHORIZED),
    };
    req.extensions_mut().insert((admin, vec!["root"]));
    req.headers_mut().insert("x-guard-ran", http::HeaderValue::from_static("yes"));
    nextrs::conventions::MiddlewareResult::next(req)
}
