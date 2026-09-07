pub async fn handle(
    mut req: http::Request<axum::body::Body>,
) -> nextrs::conventions::MiddlewareResult {
    let Some((admin, chain)) = req.extensions_mut().get_mut::<(bool, Vec<&'static str>)>() else {
        return nextrs::conventions::MiddlewareResult::response(http::StatusCode::INTERNAL_SERVER_ERROR);
    };
    if !*admin {
        return nextrs::conventions::MiddlewareResult::response(http::StatusCode::FORBIDDEN);
    }
    chain.push("admin");
    nextrs::conventions::MiddlewareResult::next(req)
}
