use askama::Template;

#[derive(Template)]
#[template(path = "with-loading/page.html")]
pub struct WithLoadingPage;

pub async fn render(_req: http::Request<axum::body::Body>) -> String {
    // Simulated slow data fetch — the loading shell shows while this awaits.
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    WithLoadingPage.render().unwrap()
}
