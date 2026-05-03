use askama::Template;
use vercel_runtime::{Error, Request, Response, ResponseBody, run, service_fn};

#[derive(Template)]
#[template(path = "page.html")]
struct IndexTemplate {
    title: String,
    heading: String,
    routes: Vec<Route>,
}

struct Route {
    path: &'static str,
    description: &'static str,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    run(service_fn(handler)).await
}

pub async fn handler(_req: Request) -> Result<Response<ResponseBody>, Error> {
    let html = IndexTemplate {
        title: "Vercel Rust + htmx".into(),
        heading: "Vercel Rust + htmx".into(),
        routes: vec![
            Route {
                path: "/readyz",
                description: "Plain-text readiness probe (200 + \"ok\").",
            },
            Route {
                path: "/landing",
                description: "Loading shell with htmx-triggered page swap (1.2s artificial delay).",
            },
        ],
    }
    .render()?;

    Ok(Response::builder()
        .status(200)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Cache-Control", "public, max-age=0, must-revalidate")
        .body(html.into())?)
}
