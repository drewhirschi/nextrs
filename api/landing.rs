use std::time::Duration;

use askama::Template;
use vercel_runtime::{Error, Request, Response, ResponseBody, run, service_fn};

#[derive(Template)]
#[template(path = "landing/loading.html")]
struct LoadingTemplate {
    swap_url: &'static str,
}

#[derive(Template)]
#[template(path = "landing/page.html")]
struct LandingPageTemplate {
    heading: String,
    body: String,
    rendered_at: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    run(service_fn(handler)).await
}

pub async fn handler(req: Request) -> Result<Response<ResponseBody>, Error> {
    if is_htmx_request(&req) {
        serve_fragment().await
    } else {
        serve_loading_shell()
    }
}

fn is_htmx_request(req: &Request) -> bool {
    req.headers()
        .get("HX-Request")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.eq_ignore_ascii_case("true"))
}

async fn serve_fragment() -> Result<Response<ResponseBody>, Error> {
    tokio::time::sleep(Duration::from_millis(1200)).await;

    let html = LandingPageTemplate {
        heading: "Landing".into(),
        body: "This content was loaded into the page after the loading shell rendered, via an htmx request to the same endpoint.".into(),
        rendered_at: chrono_like_now(),
    }
    .render()?;

    Ok(Response::builder()
        .status(200)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Cache-Control", "public, max-age=0, must-revalidate")
        .body(html.into())?)
}

fn serve_loading_shell() -> Result<Response<ResponseBody>, Error> {
    let html = LoadingTemplate {
        swap_url: "/api/landing",
    }
    .render()?;

    Ok(Response::builder()
        .status(200)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Cache-Control", "public, max-age=0, must-revalidate")
        .body(html.into())?)
}

fn chrono_like_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("epoch+{}s", now)
}
