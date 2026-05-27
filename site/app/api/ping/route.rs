use axum::body::Body;
use axum::response::IntoResponse;
use http::{Request, StatusCode};

pub async fn get(_req: Request<Body>) -> impl IntoResponse {
    StatusCode::OK
}

pub async fn post(_req: Request<Body>) -> impl IntoResponse {
    StatusCode::CREATED
}
