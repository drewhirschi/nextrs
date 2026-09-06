pub async fn handle(
    mut req: http::Request<axum::body::Body>,
) -> nextrs::conventions::MiddlewareResult {
    if req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        != Some("Bearer test")
    {
        return nextrs::conventions::MiddlewareResult::response(http::StatusCode::UNAUTHORIZED);
    }
    req.headers_mut()
        .insert("x-guard-ran", http::HeaderValue::from_static("yes"));
    nextrs::conventions::MiddlewareResult::next(req)
}
