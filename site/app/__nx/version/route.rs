/// The framework actually linked into this deployment, plus its source commit.
pub async fn get() -> impl axum::response::IntoResponse {
    (
        [("cache-control", "no-store")],
        axum::Json(serde_json::json!({
            "framework": nextrs::VERSION,
            "revision": env!("NEXTRS_BUILD_REVISION"),
        })),
    )
}
