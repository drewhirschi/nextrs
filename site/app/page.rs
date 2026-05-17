use askama::Template;

#[derive(Template)]
#[template(path = "page.html")]
pub struct HomePage;

pub async fn render(_req: http::Request<axum::body::Body>) -> String {
    HomePage.render().unwrap()
}
