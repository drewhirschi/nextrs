use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use bytes::Bytes;
use std::convert::Infallible;
use std::sync::Arc;

use crate::conventions::{LayoutFn, MiddlewareFn, MiddlewareResult, RouteEntry, RouteRegistry};

/// Internal sentinel that gets substituted into composed layouts so we can
/// split the result into the "before children" and "after children" halves.
/// Chosen to be unlikely in real content; the framework owns this string.
const NX_CONTENT_MARKER: &str = "<!--__nx_content__-->";

/// CSS id assigned to the slot div that initially holds the loading content.
const NX_SLOT_ID: &str = "__nx_slot__";

/// CSS id assigned to the `<template>` element holding the late page content.
const NX_PAGE_ID: &str = "__nx_page__";

/// Tiny inline script that swaps the loading slot for the page template.
/// Runs synchronously when its `<script>` tag is parsed — by which time both
/// the slot and the template are already in the DOM (they were streamed
/// earlier in the same response).
const NX_SWAP_SCRIPT: &str = concat!(
    "<script>(function(){",
    "var s=document.getElementById('__nx_slot__');",
    "var t=document.getElementById('__nx_page__');",
    "if(s&&t){s.replaceWith(t.content);t.remove();}",
    "})();</script>",
);

/// Collects all layouts from root down to the target path.
fn collect_layouts_for_path<'a>(entries: &'a [RouteEntry], target_path: &str) -> Vec<&'a LayoutFn> {
    let mut layouts = Vec::new();

    let mut sorted_entries: Vec<&RouteEntry> =
        entries.iter().filter(|e| e.layout.is_some()).collect();
    sorted_entries.sort_by_key(|e| route_depth(&e.path));

    for entry in sorted_entries {
        if entry_applies_to_path(&entry.path, target_path) {
            if let Some(ref layout) = entry.layout {
                layouts.push(layout);
            }
        }
    }

    layouts
}

/// Collects all middleware from root down to the target path.
fn collect_middlewares_for_path<'a>(
    entries: &'a [RouteEntry],
    target_path: &str,
) -> Vec<&'a MiddlewareFn> {
    let mut middlewares = Vec::new();

    let mut sorted_entries: Vec<&RouteEntry> =
        entries.iter().filter(|e| e.middleware.is_some()).collect();
    sorted_entries.sort_by_key(|e| route_depth(&e.path));

    for entry in sorted_entries {
        if entry_applies_to_path(&entry.path, target_path) {
            if let Some(ref middleware) = entry.middleware {
                middlewares.push(middleware);
            }
        }
    }

    middlewares
}

fn route_depth(path: &str) -> usize {
    if path == "/" {
        0
    } else {
        path.matches('/').count()
    }
}

fn entry_applies_to_path(entry_path: &str, target_path: &str) -> bool {
    entry_path == "/"
        || target_path == entry_path
        || target_path
            .strip_prefix(entry_path)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

/// Render the chain of layouts around a content marker and split the result
/// into (before_children, after_children) halves. When concatenated as
/// `before + content + after`, the result is identical to applying the layouts
/// directly around `content`.
fn layout_shell(layouts: &[&LayoutFn]) -> (String, String) {
    let mut shell = NX_CONTENT_MARKER.to_string();
    // Innermost layout wraps first; outer layouts wrap the result.
    for layout in layouts.iter().rev() {
        shell = layout(&shell);
    }
    let mut parts = shell.splitn(2, NX_CONTENT_MARKER);
    let before = parts.next().unwrap_or("").to_string();
    let after = parts.next().unwrap_or("").to_string();
    (before, after)
}

/// Build an Axum router from a [`RouteRegistry`], with a `public/` directory
/// served as a fallback for paths the router doesn't match.
///
/// If `public_dir` exists, requests that miss every route are served from it
/// via `tower-http::services::ServeDir`. If the directory doesn't exist this
/// is equivalent to [`build_router`] — useful for crates that may not have any
/// static assets yet.
///
/// On Vercel this fallback is irrelevant: the CDN matches static files
/// *before* the catch-all rewrite to the function. This helper exists so dev
/// (where the function-equivalent is `cargo run`) resolves the same URLs the
/// same way as production.
pub fn build_router_with_public(
    registry: RouteRegistry,
    public_dir: impl AsRef<std::path::Path>,
) -> Router {
    let router = build_router(registry);
    let path = public_dir.as_ref();
    if path.is_dir() {
        router.fallback_service(tower_http::services::ServeDir::new(path))
    } else {
        router
    }
}

/// Build an Axum router from a [`RouteRegistry`].
pub fn build_router(registry: RouteRegistry) -> Router {
    let entries = Arc::new(registry.entries);
    let mut router = Router::new();

    for i in 0..entries.len() {
        let entries_clone = Arc::clone(&entries);
        let path = entries[i].path.clone();

        let has_page = entries[i].page.is_some();
        let has_methods = !entries[i].methods.is_empty();

        if has_page {
            let entries_for_get = Arc::clone(&entries_clone);
            let path_for_get = path.clone();
            let idx = i;

            router = router.route(
                &path,
                get(move |req: Request| {
                    let entries = Arc::clone(&entries_for_get);
                    let path = path_for_get.clone();
                    async move { render_route(entries, idx, path, req).await }
                }),
            );
        }

        if has_methods {
            for (j, (method, _)) in entries[i].methods.iter().enumerate() {
                let entries_for_method = Arc::clone(&entries_clone);
                let method_clone = method.clone();
                let idx = i;

                let path_for_method = path.clone();

                let handler = move |req: Request| {
                    let entries = Arc::clone(&entries_for_method);
                    let path = path_for_method.clone();
                    async move { handle_method_route(entries, idx, j, path, req).await }
                };

                router = router.route(
                    &path,
                    axum::routing::on(method_to_filter(&method_clone), handler),
                );
            }
        }
    }

    router
}

async fn render_route(
    entries: Arc<Vec<RouteEntry>>,
    idx: usize,
    path: String,
    req: Request,
) -> Response {
    let req = match run_middlewares(&entries, &path, req).await {
        Ok(req) => req,
        Err(response) => return response,
    };

    let layouts = collect_layouts_for_path(&entries, &path);
    let (before, after) = layout_shell(&layouts);

    let has_loading = entries[idx].loading.is_some();

    if has_loading {
        let loading_html = entries[idx].loading.as_ref().unwrap()();
        let slot_div = format!(r#"<div id="{}">{}</div>"#, NX_SLOT_ID, loading_html);

        let stream = async_stream::stream! {
            yield Ok::<Bytes, Infallible>(Bytes::from(before));
            yield Ok(Bytes::from(slot_div));

            let page_html = entries[idx].page.as_ref().unwrap()(req).await;
            let swap_chunk = format!(
                r#"<template id="{}">{}</template>{}"#,
                NX_PAGE_ID, page_html, NX_SWAP_SCRIPT,
            );
            yield Ok(Bytes::from(swap_chunk));

            yield Ok(Bytes::from(after));
        };

        Response::builder()
            .header("content-type", "text/html; charset=utf-8")
            .body(Body::from_stream(stream))
            .unwrap()
    } else {
        let page_html = entries[idx].page.as_ref().unwrap()(req).await;
        let full = format!("{}{}{}", before, page_html, after);
        Html(full).into_response()
    }
}

async fn handle_method_route(
    entries: Arc<Vec<RouteEntry>>,
    idx: usize,
    method_idx: usize,
    path: String,
    req: Request,
) -> Response {
    let req = match run_middlewares(&entries, &path, req).await {
        Ok(req) => req,
        Err(response) => return response,
    };

    let route_fn = &entries[idx].methods[method_idx].1;
    route_fn(req).await
}

async fn run_middlewares(
    entries: &[RouteEntry],
    path: &str,
    mut req: Request,
) -> Result<Request, Response> {
    for middleware in collect_middlewares_for_path(entries, path) {
        match middleware(req).await {
            MiddlewareResult::Continue(next_req) => req = next_req,
            MiddlewareResult::Response(response) => return Err(response),
        }
    }

    Ok(req)
}

fn method_to_filter(method: &http::Method) -> axum::routing::MethodFilter {
    match *method {
        http::Method::GET => axum::routing::MethodFilter::GET,
        http::Method::POST => axum::routing::MethodFilter::POST,
        http::Method::PUT => axum::routing::MethodFilter::PUT,
        http::Method::DELETE => axum::routing::MethodFilter::DELETE,
        http::Method::PATCH => axum::routing::MethodFilter::PATCH,
        http::Method::HEAD => axum::routing::MethodFilter::HEAD,
        http::Method::OPTIONS => axum::routing::MethodFilter::OPTIONS,
        _ => axum::routing::MethodFilter::GET,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conventions::{
        RouteEntry, RouteRegistry, static_layout, static_loading, static_page,
    };
    use axum::body::Body;
    use http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    async fn body_to_string(body: Body) -> String {
        let bytes = body.collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn dyn_page(content: &'static str) -> crate::conventions::PageFn {
        Box::new(move |_req| Box::pin(async move { content.to_string() }))
    }

    fn slow_dyn_page(content: &'static str, ms: u64) -> crate::conventions::PageFn {
        Box::new(move |_req| {
            Box::pin(async move {
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                content.to_string()
            })
        })
    }

    fn dyn_layout(class: &'static str) -> crate::conventions::LayoutFn {
        Box::new(move |children| format!("<div class=\"{}\">{}</div>", class, children))
    }

    // -- Round 2 / synchronous render -----------------------------------------

    #[tokio::test]
    async fn build_router_with_public_serves_static_files_on_route_miss() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("hello.txt"), "hi from public").unwrap();

        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: Some(dyn_page("home")),
            layout: None,
            loading: None,
            middleware: None,
            methods: vec![],
        });

        let app = build_router_with_public(registry, tmp.path());

        // Route hit still wins.
        let resp = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_to_string(resp.into_body()).await, "home");

        // Path with no matching route falls through to ServeDir.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/hello.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_to_string(resp.into_body()).await, "hi from public");
    }

    #[tokio::test]
    async fn build_router_with_public_skips_serve_dir_when_path_missing() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: Some(dyn_page("home")),
            layout: None,
            loading: None,
            middleware: None,
            methods: vec![],
        });

        let app = build_router_with_public(registry, "/nonexistent/path/for/test");
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/missing.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_page_without_loading_returns_content() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: Some(dyn_page("<h1>Home</h1>")),
            layout: None,
            loading: None,
            middleware: None,
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_to_string(resp.into_body()).await, "<h1>Home</h1>");
    }

    #[tokio::test]
    async fn test_layout_wraps_page_content() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: Some(dyn_page("<h1>Home</h1>")),
            layout: Some(dyn_layout("root")),
            loading: None,
            middleware: None,
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(
            body_to_string(resp.into_body()).await,
            "<div class=\"root\"><h1>Home</h1></div>",
        );
    }

    #[tokio::test]
    async fn test_nested_layouts_compose() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: None,
            layout: Some(dyn_layout("root")),
            loading: None,
            middleware: None,
            methods: vec![],
        });
        registry.add(RouteEntry {
            path: "/dashboard".to_string(),
            page: Some(dyn_page("<h1>Dash</h1>")),
            layout: Some(dyn_layout("dash")),
            loading: None,
            middleware: None,
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/dashboard")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            body_to_string(resp.into_body()).await,
            "<div class=\"root\"><div class=\"dash\"><h1>Dash</h1></div></div>",
        );
    }

    #[tokio::test]
    async fn test_three_level_layout_nesting() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: None,
            layout: Some(dyn_layout("root")),
            loading: None,
            middleware: None,
            methods: vec![],
        });
        registry.add(RouteEntry {
            path: "/dashboard".to_string(),
            page: None,
            layout: Some(dyn_layout("dash")),
            loading: None,
            middleware: None,
            methods: vec![],
        });
        registry.add(RouteEntry {
            path: "/dashboard/settings".to_string(),
            page: Some(dyn_page("settings-page")),
            layout: Some(dyn_layout("settings")),
            loading: None,
            middleware: None,
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/dashboard/settings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            body_to_string(resp.into_body()).await,
            "<div class=\"root\"><div class=\"dash\"><div class=\"settings\">settings-page</div></div></div>",
        );
    }

    #[tokio::test]
    async fn test_dynamic_path_segments() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/users/{id}".to_string(),
            page: Some(dyn_page("<h1>User Profile</h1>")),
            layout: None,
            loading: None,
            middleware: None,
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/users/42")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            body_to_string(resp.into_body()).await,
            "<h1>User Profile</h1>"
        );
    }

    #[tokio::test]
    async fn test_multiple_routes() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: Some(dyn_page("home")),
            layout: None,
            loading: None,
            middleware: None,
            methods: vec![],
        });
        registry.add(RouteEntry {
            path: "/about".to_string(),
            page: Some(dyn_page("about")),
            layout: None,
            loading: None,
            middleware: None,
            methods: vec![],
        });

        let app = build_router(registry);

        let resp1 = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(body_to_string(resp1.into_body()).await, "home");

        let resp2 = app
            .oneshot(
                Request::builder()
                    .uri("/about")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(body_to_string(resp2.into_body()).await, "about");
    }

    // -- Round 2: static (`.html`) handlers compose with the framework --------

    #[tokio::test]
    async fn test_static_page_renders() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/about".to_string(),
            page: Some(static_page("<h1>About</h1>")),
            layout: None,
            loading: None,
            middleware: None,
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/about")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(body_to_string(resp.into_body()).await, "<h1>About</h1>");
    }

    #[tokio::test]
    async fn test_static_layout_wraps_static_page() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: Some(static_page("<h1>Home</h1>")),
            layout: Some(static_layout("<html><body>{{children}}</body></html>")),
            loading: None,
            middleware: None,
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(
            body_to_string(resp.into_body()).await,
            "<html><body><h1>Home</h1></body></html>",
        );
    }

    #[tokio::test]
    async fn test_layout_shell_split_around_marker() {
        let layouts: Vec<crate::conventions::LayoutFn> = vec![
            static_layout("<html>{{children}}</html>"),
            static_layout("<body>{{children}}</body>"),
        ];
        let refs: Vec<&crate::conventions::LayoutFn> = layouts.iter().collect();
        let (before, after) = layout_shell(&refs);
        assert_eq!(before, "<html><body>");
        assert_eq!(after, "</body></html>");
    }

    // -- Round 4: mixed static / dynamic compose correctly --------------------

    #[tokio::test]
    async fn test_static_layout_with_dynamic_page() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: Some(dyn_page("<h1>Hi</h1>")),
            layout: Some(static_layout("<main>{{children}}</main>")),
            loading: None,
            middleware: None,
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(
            body_to_string(resp.into_body()).await,
            "<main><h1>Hi</h1></main>",
        );
    }

    #[tokio::test]
    async fn test_dynamic_layout_with_static_page() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: Some(static_page("<h1>Hi</h1>")),
            layout: Some(dyn_layout("root")),
            loading: None,
            middleware: None,
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(
            body_to_string(resp.into_body()).await,
            "<div class=\"root\"><h1>Hi</h1></div>",
        );
    }

    #[tokio::test]
    async fn test_mixed_static_and_dynamic_nested_layouts() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: None,
            layout: Some(static_layout("<html>{{children}}</html>")),
            loading: None,
            middleware: None,
            methods: vec![],
        });
        registry.add(RouteEntry {
            path: "/dashboard".to_string(),
            page: Some(dyn_page("page")),
            layout: Some(dyn_layout("dash")),
            loading: None,
            middleware: None,
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/dashboard")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            body_to_string(resp.into_body()).await,
            "<html><div class=\"dash\">page</div></html>",
        );
    }

    // -- Streaming (loading) --------------------------------------------------

    #[tokio::test]
    async fn test_loading_stream_contains_loading_then_page() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/dashboard".to_string(),
            page: Some(slow_dyn_page("<h1>Dashboard</h1>", 30)),
            layout: Some(static_layout("<html>{{children}}</html>")),
            loading: Some(static_loading("<p>Loading...</p>")),
            middleware: None,
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/dashboard")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = body_to_string(resp.into_body()).await;

        // All the expected pieces appear in the right order
        let layout_open = body.find("<html>").expect("layout open");
        let slot_div = body.find("__nx_slot__").expect("slot div");
        let loading = body.find("Loading...").expect("loading text");
        let page_template = body.find("__nx_page__").expect("page template");
        let dashboard = body.find("<h1>Dashboard</h1>").expect("page content");
        let swap_script = body.find("<script>").expect("swap script");
        let layout_close = body.find("</html>").expect("layout close");

        assert!(layout_open < slot_div);
        assert!(slot_div < loading);
        assert!(loading < page_template);
        assert!(page_template < dashboard);
        assert!(dashboard < swap_script);
        assert!(swap_script < layout_close);
    }

    #[tokio::test]
    async fn test_loading_stream_yields_multiple_frames() {
        // Confirm the response is actually streamed (multiple body frames),
        // not coalesced into a single chunk before the page resolved.
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/dashboard".to_string(),
            page: Some(slow_dyn_page("<h1>D</h1>", 30)),
            layout: None,
            loading: Some(static_loading("L")),
            middleware: None,
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/dashboard")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let mut body = resp.into_body();
        let mut frames = 0;
        while let Some(Ok(_)) = http_body_util::BodyExt::frame(&mut body).await {
            frames += 1;
        }
        assert!(
            frames > 1,
            "expected streamed body, got {} frame(s)",
            frames
        );
    }

    #[tokio::test]
    async fn test_loading_arrives_before_page_resolves() {
        // The whole point of streaming is that the loading shell reaches the
        // client BEFORE the page handler finishes. We verify by reading body
        // frames with timestamps and asserting:
        //   1. The loading shell shows up well before the page sleep elapses.
        //   2. The page content shows up only after the sleep — i.e. the
        //      response is genuinely incremental, not buffered.
        use std::time::{Duration, Instant};

        let page_sleep_ms = 200;
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/dashboard".to_string(),
            page: Some(slow_dyn_page("PAGE_CONTENT", page_sleep_ms)),
            layout: None,
            loading: Some(static_loading("LOADING_SHELL")),
            middleware: None,
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/dashboard")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let start = Instant::now();
        let mut body = resp.into_body();

        // Drain frames, capturing each frame's content and the time it arrived.
        let mut frames: Vec<(Duration, String)> = Vec::new();
        while let Some(Ok(frame)) = http_body_util::BodyExt::frame(&mut body).await {
            if let Ok(data) = frame.into_data() {
                let elapsed = start.elapsed();
                let text = String::from_utf8(data.to_vec()).unwrap();
                frames.push((elapsed, text));
            }
        }

        assert!(
            frames.len() >= 2,
            "expected ≥2 frames, got {}: {:?}",
            frames.len(),
            frames
        );

        // The loading shell must appear in some frame that arrived BEFORE the
        // sleep elapsed — that's the proof of incremental streaming.
        let loading_arrival = frames
            .iter()
            .find(|(_, text)| text.contains("LOADING_SHELL"))
            .map(|(t, _)| *t)
            .expect("no frame contained LOADING_SHELL");
        assert!(
            loading_arrival < Duration::from_millis(page_sleep_ms / 2),
            "loading shell arrived at {}ms, expected <{}ms (page_sleep_ms/2)",
            loading_arrival.as_millis(),
            page_sleep_ms / 2,
        );

        // The page content must appear in some frame that arrived AFTER the
        // sleep — proves we're actually waiting on the page, not pre-buffering.
        let page_arrival = frames
            .iter()
            .find(|(_, text)| text.contains("PAGE_CONTENT"))
            .map(|(t, _)| *t)
            .expect("no frame contained PAGE_CONTENT");
        assert!(
            page_arrival >= Duration::from_millis(page_sleep_ms),
            "page chunk arrived at {}ms, expected ≥{}ms (after page sleep)",
            page_arrival.as_millis(),
            page_sleep_ms,
        );
    }

    #[tokio::test]
    async fn test_loading_with_nested_layouts() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: None,
            layout: Some(dyn_layout("root")),
            loading: None,
            middleware: None,
            methods: vec![],
        });
        registry.add(RouteEntry {
            path: "/dashboard".to_string(),
            page: Some(slow_dyn_page("<h1>D</h1>", 10)),
            layout: Some(dyn_layout("dash")),
            loading: Some(static_loading("loading-shell")),
            middleware: None,
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/dashboard")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = body_to_string(resp.into_body()).await;

        // Both layouts wrap the loading shell; the page swap arrives later.
        assert!(body.contains("<div class=\"root\"><div class=\"dash\">"));
        assert!(body.contains("loading-shell"));
        assert!(body.contains("<h1>D</h1>"));
        // Layout close happens after everything.
        assert!(body.ends_with("</div></div>"));
    }

    // -- Middleware -----------------------------------------------------------

    #[tokio::test]
    async fn test_middleware_redirect_prevents_loading_stream() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/reviews".to_string(),
            page: Some(slow_dyn_page("PAGE_CONTENT", 30)),
            layout: None,
            loading: Some(static_loading("LOADING_SHELL")),
            middleware: Some(Box::new(|_req| {
                Box::pin(async {
                    MiddlewareResult::response(axum::response::Redirect::to("/auth/login"))
                })
            })),
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/reviews")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            resp.headers().get(http::header::LOCATION).unwrap(),
            "/auth/login",
        );
        let body = body_to_string(resp.into_body()).await;
        assert!(!body.contains("LOADING_SHELL"));
        assert!(!body.contains("PAGE_CONTENT"));
    }

    #[tokio::test]
    async fn test_middleware_continue_preserves_loading_stream() {
        use std::time::{Duration, Instant};

        let page_sleep_ms = 200;
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/reviews".to_string(),
            page: Some(slow_dyn_page("PAGE_CONTENT", page_sleep_ms)),
            layout: None,
            loading: Some(static_loading("LOADING_SHELL")),
            middleware: Some(Box::new(|req| {
                Box::pin(async { MiddlewareResult::next(req) })
            })),
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/reviews")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let start = Instant::now();
        let mut body = resp.into_body();
        let mut frames: Vec<(Duration, String)> = Vec::new();
        while let Some(Ok(frame)) = http_body_util::BodyExt::frame(&mut body).await {
            if let Ok(data) = frame.into_data() {
                frames.push((start.elapsed(), String::from_utf8(data.to_vec()).unwrap()));
            }
        }

        let loading_arrival = frames
            .iter()
            .find(|(_, text)| text.contains("LOADING_SHELL"))
            .map(|(t, _)| *t)
            .expect("no frame contained LOADING_SHELL");
        assert!(loading_arrival < Duration::from_millis(page_sleep_ms / 2));

        let page_arrival = frames
            .iter()
            .find(|(_, text)| text.contains("PAGE_CONTENT"))
            .map(|(t, _)| *t)
            .expect("no frame contained PAGE_CONTENT");
        assert!(page_arrival >= Duration::from_millis(page_sleep_ms));
    }

    #[tokio::test]
    async fn test_nested_middlewares_run_root_to_leaf() {
        let order = Arc::new(Mutex::new(Vec::new()));

        let root_order = Arc::clone(&order);
        let dashboard_order = Arc::clone(&order);
        let settings_order = Arc::clone(&order);

        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: None,
            layout: None,
            loading: None,
            middleware: Some(Box::new(move |req| {
                let order = Arc::clone(&root_order);
                Box::pin(async move {
                    order.lock().unwrap().push("root");
                    MiddlewareResult::next(req)
                })
            })),
            methods: vec![],
        });
        registry.add(RouteEntry {
            path: "/dashboard".to_string(),
            page: None,
            layout: None,
            loading: None,
            middleware: Some(Box::new(move |req| {
                let order = Arc::clone(&dashboard_order);
                Box::pin(async move {
                    order.lock().unwrap().push("dashboard");
                    MiddlewareResult::next(req)
                })
            })),
            methods: vec![],
        });
        registry.add(RouteEntry {
            path: "/dashboard/settings".to_string(),
            page: Some(dyn_page("settings")),
            layout: None,
            loading: None,
            middleware: Some(Box::new(move |req| {
                let order = Arc::clone(&settings_order);
                Box::pin(async move {
                    order.lock().unwrap().push("settings");
                    MiddlewareResult::next(req)
                })
            })),
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/dashboard/settings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(body_to_string(resp.into_body()).await, "settings");
        assert_eq!(
            *order.lock().unwrap(),
            vec!["root", "dashboard", "settings"],
        );
    }

    #[tokio::test]
    async fn test_middleware_can_modify_request_for_page() {
        #[derive(Clone)]
        struct Tenant(&'static str);

        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/reviews".to_string(),
            page: Some(Box::new(|req| {
                Box::pin(async move {
                    req.extensions()
                        .get::<Tenant>()
                        .map(|tenant| tenant.0)
                        .unwrap_or("missing")
                        .to_string()
                })
            })),
            layout: None,
            loading: None,
            middleware: Some(Box::new(|mut req| {
                Box::pin(async move {
                    req.extensions_mut().insert(Tenant("acme"));
                    MiddlewareResult::next(req)
                })
            })),
            methods: vec![],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/reviews")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(body_to_string(resp.into_body()).await, "acme");
    }

    // -- API routes -----------------------------------------------------------

    #[tokio::test]
    async fn test_route_methods() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/api/users".to_string(),
            page: None,
            layout: None,
            loading: None,
            middleware: None,
            methods: vec![(
                http::Method::POST,
                Box::new(|_req| {
                    Box::pin(async { (StatusCode::CREATED, "created").into_response() })
                }),
            )],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/users")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
        assert_eq!(body_to_string(resp.into_body()).await, "created");
    }

    #[tokio::test]
    async fn test_page_and_route_on_same_path() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/contact".to_string(),
            page: Some(dyn_page("<form>contact form</form>")),
            layout: None,
            loading: None,
            middleware: None,
            methods: vec![(
                http::Method::POST,
                Box::new(|_req| {
                    Box::pin(async { (StatusCode::OK, "form submitted").into_response() })
                }),
            )],
        });

        let app = build_router(registry);

        let get_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/contact")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            body_to_string(get_resp.into_body()).await,
            "<form>contact form</form>",
        );

        let post_resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/contact")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            body_to_string(post_resp.into_body()).await,
            "form submitted"
        );
    }

    #[tokio::test]
    async fn test_middleware_applies_to_route_methods() {
        let handler_called = Arc::new(AtomicBool::new(false));
        let handler_called_for_route = Arc::clone(&handler_called);

        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/api/reviews".to_string(),
            page: None,
            layout: None,
            loading: None,
            middleware: Some(Box::new(|_req| {
                Box::pin(async { MiddlewareResult::response(StatusCode::UNAUTHORIZED) })
            })),
            methods: vec![(
                http::Method::POST,
                Box::new(move |_req| {
                    let handler_called = Arc::clone(&handler_called_for_route);
                    Box::pin(async move {
                        handler_called.store(true, Ordering::SeqCst);
                        (StatusCode::CREATED, "created").into_response()
                    })
                }),
            )],
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/reviews")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(!handler_called.load(Ordering::SeqCst));
    }
}
