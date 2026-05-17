use askama::Template;

#[derive(Template)]
#[template(path = "simple/page.html")]
pub struct SimplePage;

pub async fn render(_req: http::Request<axum::body::Body>) -> String {
    SimplePage.render().unwrap()
}
