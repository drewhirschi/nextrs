use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use bytes::Bytes;
use std::convert::Infallible;
use std::sync::Arc;

use crate::conventions::{
    LayoutFn, MiddlewareFn, MiddlewareResult, NotFoundEntry, RouteEntry, RouteRegistry,
};
use crate::speculation::{ResolvedSpeculation, SpeculationConfig};

/// Internal sentinel that gets substituted into composed layouts so we can
/// split the result into the "before children" and "after children" halves.
/// Chosen to be unlikely in real content; the framework owns this string.
const NX_CONTENT_MARKER: &str = "<!--__nx_content__-->";

/// CSS id assigned to the slot div that initially holds the loading content.
const NX_SLOT_ID: &str = "__nx_slot__";

/// CSS id assigned to the `<template>` element holding the late page content.
const NX_PAGE_ID: &str = "__nx_page__";

/// Inline script that swaps the loading slot for the page template. Script
/// elements moved out of a `<template>` are inert in browsers, so the swapper
/// recreates them before insertion. That lets streamed React pages execute
/// their final page bundle after `prefetch.rs` resolves.
const NX_SWAP_SCRIPT: &str = concat!(
    "<script>(function(){",
    "var s=document.getElementById('__nx_slot__');",
    "var t=document.getElementById('__nx_page__');",
    "if(!s||!t)return;",
    "var c=t.content.cloneNode(true);",
    "c.querySelectorAll('script').forEach(function(o){",
    "var n=document.createElement('script');",
    "for(var i=0;i<o.attributes.length;i++){var a=o.attributes[i];n.setAttribute(a.name,a.value);}",
    "n.text=o.text;",
    "o.replaceWith(n);",
    "});",
    "s.replaceWith(c);",
    "t.remove();",
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
    build_router_with_public_and_speculation(registry, public_dir, SpeculationConfig::default())
}

/// [`build_router_with_public`] with an explicit [`SpeculationConfig`] — the
/// opt-in for server-rendered apps that also serve a `public/` directory. See
/// [`build_router_with_speculation`] for what the config controls.
pub fn build_router_with_public_and_speculation(
    registry: RouteRegistry,
    public_dir: impl AsRef<std::path::Path>,
    speculation: SpeculationConfig,
) -> Router {
    return build_router_with_public_inner(registry, public_dir, speculation)
        .layer(axum::middleware::map_response(crate::health::stamp));
}

fn build_router_with_public_inner(
    registry: RouteRegistry,
    public_dir: impl AsRef<std::path::Path>,
    speculation: SpeculationConfig,
) -> Router {
    use axum::handler::HandlerWithoutStateExt;

    // Build the route table on its own so the not-found surfaces can be wired
    // as the fallback below.
    let speculation = Arc::new(speculation.resolve(&registry.react_pages));
    let entries = Arc::new(registry.entries);
    let not_found = Arc::new(registry.not_found);
    let router = build_route_table(Arc::clone(&entries), Arc::clone(&speculation));
    let path = public_dir.as_ref();

    let router = if path.is_dir() {
        // ServeDir is tried for unmatched paths; when it misses, fall through to
        // the not-found surfaces (if any) rather than ServeDir's bare 404.
        if not_found.is_empty() {
            router
                .fallback_service(tower_http::services::ServeDir::new(path))
                .layer(axum::middleware::map_response(declare_utf8_charset))
        } else {
            let nf_entries = Arc::clone(&entries);
            let nf_list = Arc::clone(&not_found);
            let nf_speculation = Arc::clone(&speculation);
            let nf_handler = move |req: Request| {
                let entries = Arc::clone(&nf_entries);
                let not_found = Arc::clone(&nf_list);
                let speculation = Arc::clone(&nf_speculation);
                async move { render_not_found(&entries, &not_found, &speculation, req).await }
            };
            let serve = tower_http::services::ServeDir::new(path)
                .not_found_service(nf_handler.into_service());
            router
                .fallback_service(serve)
                .layer(axum::middleware::map_response(declare_utf8_charset))
        }
    } else {
        with_not_found_fallback(router, entries, not_found, speculation)
    };

    router.layer(axum::middleware::from_fn(generated_asset_cache))
}

/// Generated assets are content-addressed in production and deliberately
/// uncached in development. This covers local/Docker ServeDir; deployment
/// adapters whose static CDN bypasses Axum must install the same `/dist/*`
/// policy in their platform configuration.
async fn generated_asset_cache(req: Request, next: axum::middleware::Next) -> Response {
    let generated = req.uri().path().starts_with("/dist/");
    let mut response = next.run(req).await;
    if generated && response.status().is_success() {
        response.headers_mut().insert(
            http::header::CACHE_CONTROL,
            http::HeaderValue::from_static(generated_asset_cache_control(cfg!(debug_assertions))),
        );
    }
    response
}

fn generated_asset_cache_control(debug: bool) -> &'static str {
    if debug {
        "no-store"
    } else {
        "public, max-age=31536000, immutable"
    }
}

/// ServeDir guesses mime types without a charset (`text/plain`, not
/// `text/plain; charset=utf-8`), and browsers fall back to Latin-1 for bare
/// text types — garbling any non-ASCII byte. Everything this server produces
/// is UTF-8 (Rust strings, files we author), so say so.
async fn declare_utf8_charset(mut resp: Response) -> Response {
    let Some(ct) = resp.headers().get(http::header::CONTENT_TYPE) else {
        return resp;
    };
    let Ok(ct) = ct.to_str() else { return resp };
    if ct.starts_with("text/") && !ct.contains("charset") {
        let with_charset = format!("{}; charset=utf-8", ct);
        if let Ok(value) = http::HeaderValue::from_str(&with_charset) {
            resp.headers_mut().insert(http::header::CONTENT_TYPE, value);
        }
    }
    resp
}

/// Build an Axum router from a [`RouteRegistry`]. Document-level speculation
/// is off by default (changed in 0.4.0) — see [`build_router_with_speculation`]
/// to opt a server-rendered app in.
pub fn build_router(registry: RouteRegistry) -> Router {
    build_router_with_speculation(registry, SpeculationConfig::default())
}

/// Build an Axum router from a [`RouteRegistry`] with an explicit
/// [`SpeculationConfig`].
///
/// When enabled, the framework injects a `<script type="speculationrules">`
/// into the `<head>` of every full-document page response so same-origin links
/// are speculatively fetched (or prerendered) by the browser — no client-side
/// JS. React app-shell routes ([`RouteRegistry::react_pages`]) are excluded
/// from the rules: the shell soft-navigates them, so a speculated document
/// would never be used. See [`crate::speculation`] for the full picture
/// (prefetch vs preload vs speculation).
pub fn build_router_with_speculation(
    registry: RouteRegistry,
    speculation: SpeculationConfig,
) -> Router {
    let speculation = Arc::new(speculation.resolve(&registry.react_pages));
    let entries = Arc::new(registry.entries);
    let not_found = Arc::new(registry.not_found);
    let router = build_route_table(Arc::clone(&entries), Arc::clone(&speculation));
    with_not_found_fallback(router, entries, not_found, speculation)
        .layer(axum::middleware::map_response(crate::health::stamp))
}

/// Deprecated name for [`build_router_with_speculation`].
#[deprecated(
    note = "renamed to build_router_with_speculation — this only controls document-level Speculation Rules, not data prefetch"
)]
pub fn build_router_with_prefetch(registry: RouteRegistry, prefetch: SpeculationConfig) -> Router {
    build_router_with_speculation(registry, prefetch)
}

/// Reserved path of the soft-nav data-prefetch endpoint. The generated app
/// shell calls `GET /__nx/prefetch?path=<url>` from its route loaders (on
/// hover via preload, and during navigation) and hydrates the React Query
/// cache with the response — the same entries a hard load of `<url>` would
/// stream as `__nx_seeds__`.
pub const NX_PREFETCH_PATH: &str = "/__nx/prefetch";

/// The endpoint dispatches into an internal router that mirrors every
/// prefetch-capable route at its real pattern — reusing axum's matcher means
/// params and wildcards resolve exactly like a page render, and the route's
/// middleware chain runs first (a protected page's data stays protected).
fn build_prefetch_endpoint(entries: Arc<Vec<RouteEntry>>) -> Router {
    let mut inner = Router::new();
    for i in 0..entries.len() {
        if entries[i].prefetch.is_none() {
            continue;
        }
        let entries_for_route = Arc::clone(&entries);
        let path = entries[i].path.clone();
        let idx = i;
        let route_path = path.clone();
        inner = inner.route(
            &path,
            get(move |req: Request| {
                let entries = Arc::clone(&entries_for_route);
                let path = route_path.clone();
                async move {
                    let req = match run_middlewares(&entries, &path, req).await {
                        Ok(req) => req,
                        Err(response) => return response,
                    };
                    let seeds = entries[idx].prefetch.as_ref().unwrap()(req).await;
                    axum::Json(seeds.to_json()).into_response()
                }
            }),
        );
    }

    let inner = Arc::new(inner);
    Router::new().route(
        NX_PREFETCH_PATH,
        get(move |req: Request| {
            let inner = Arc::clone(&inner);
            async move {
                let target = req
                    .uri()
                    .query()
                    .and_then(|q| serde_urlencoded::from_str::<Vec<(String, String)>>(q).ok())
                    .and_then(|pairs| pairs.into_iter().find(|(k, _)| k == "path"))
                    .map(|(_, v)| v);
                // Same-app paths only: absolute-path form, no scheme/authority.
                let Some(target) = target.filter(|t| t.starts_with('/') && !t.starts_with("//"))
                else {
                    return http::StatusCode::BAD_REQUEST.into_response();
                };
                let Ok(uri) = target.parse::<http::Uri>() else {
                    return http::StatusCode::BAD_REQUEST.into_response();
                };
                // Re-target the ORIGINAL request (headers intact — cookies and
                // auth flow through the middleware chain unchanged).
                let (mut parts, body) = req.into_parts();
                parts.uri = uri;
                use tower::util::ServiceExt;
                match (*inner)
                    .clone()
                    .oneshot(Request::from_parts(parts, body))
                    .await
                {
                    Ok(response) => response,
                    Err(never) => match never {},
                }
            }
        }),
    )
}

/// Build just the route table (matched routes, no fallback) from the registry's
/// entries, with the resolved speculation script threaded into page rendering.
fn build_route_table(entries: Arc<Vec<RouteEntry>>, speculation: Arc<ResolvedSpeculation>) -> Router {
    let mut router = Router::new();

    for i in 0..entries.len() {
        let entries_clone = Arc::clone(&entries);
        let path = entries[i].path.clone();

        let has_page = entries[i].page.is_some();
        let has_methods = !entries[i].methods.is_empty();

        if has_page {
            let entries_for_get = Arc::clone(&entries_clone);
            let speculation_for_get = Arc::clone(&speculation);
            let path_for_get = path.clone();
            let idx = i;

            router = router.route(
                &path,
                get(move |req: Request| {
                    let entries = Arc::clone(&entries_for_get);
                    let speculation = Arc::clone(&speculation_for_get);
                    let path = path_for_get.clone();
                    async move { render_route(entries, speculation, idx, path, req).await }
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

    // Soft-nav data prefetch: only mounted when some route can serve it, so
    // apps without prefetch-backed React pages don't reserve the path.
    if entries.iter().any(|e| e.prefetch.is_some()) {
        router = router.merge(build_prefetch_endpoint(Arc::clone(&entries)));
    }

    // Fleet-uniform start-temperature telemetry; also anchors uptime_ms to
    // router construction (≈ process boot).
    crate::health::init();
    router = router.route(crate::health::NX_HEALTH_PATH, get(crate::health::handler));

    router
}

/// Install the not-found surfaces as the router's fallback, if any exist.
/// With none registered, the router keeps Axum's default bare `404`.
fn with_not_found_fallback(
    router: Router,
    entries: Arc<Vec<RouteEntry>>,
    not_found: Arc<Vec<NotFoundEntry>>,
    speculation: Arc<ResolvedSpeculation>,
) -> Router {
    if not_found.is_empty() {
        return router;
    }
    router.fallback(move |req: Request| {
        let entries = Arc::clone(&entries);
        let not_found = Arc::clone(&not_found);
        let speculation = Arc::clone(&speculation);
        async move { render_not_found(&entries, &not_found, &speculation, req).await }
    })
}

/// Render the not-found surface for an unmatched request. Picks the entry whose
/// declaring path is the *deepest* ancestor of the requested path (so
/// `/admin/x` prefers an `/admin` not-found over the root one), wraps it in that
/// segment's layouts, and responds `404`. Falls back to a bare `404` when no
/// not-found surface covers the path.
///
/// Per-directory middleware does not run here — middleware is scoped to matched
/// routes, and an unmatched path matched none.
async fn render_not_found(
    entries: &[RouteEntry],
    not_found: &[NotFoundEntry],
    speculation: &ResolvedSpeculation,
    req: Request,
) -> Response {
    let path = req.uri().path().to_string();
    let chosen = not_found
        .iter()
        .filter(|nf| entry_applies_to_path(&nf.path, &path))
        .max_by_key(|nf| route_depth(&nf.path));

    let Some(nf) = chosen else {
        return http::StatusCode::NOT_FOUND.into_response();
    };

    let layouts = collect_layouts_for_path(entries, &nf.path);
    let (before, after) = layout_shell(&layouts);
    // Same speculation-rules injection as a normal page response (no-op for
    // head-less fragments / when speculation is off).
    let before = speculation.inject_into_head(before);
    let body = (nf.render)(req).await;
    let full = format!("{}{}{}", before, body, after);

    (http::StatusCode::NOT_FOUND, Html(full)).into_response()
}

async fn render_route(
    entries: Arc<Vec<RouteEntry>>,
    speculation: Arc<ResolvedSpeculation>,
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
    // Inject the speculation-rules <script> into <head>. A no-op for head-less
    // fragments and when speculation is off, so both the streaming and
    // non-streaming branches below can use `before` directly.
    let before = speculation.inject_into_head(before);

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
            prefetch: None,
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
    async fn generated_assets_receive_the_current_profile_policy() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("dist")).unwrap();
        std::fs::write(tmp.path().join("dist/app-deadbeef.js"), "export {};").unwrap();

        let app = build_router_with_public(RouteRegistry::new(), tmp.path());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/dist/app-deadbeef.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()[http::header::CACHE_CONTROL],
            generated_asset_cache_control(cfg!(debug_assertions))
        );
    }

    #[test]
    fn generated_asset_policies_cover_development_and_production() {
        assert_eq!(generated_asset_cache_control(true), "no-store");
        assert_eq!(
            generated_asset_cache_control(false),
            "public, max-age=31536000, immutable"
        );
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
            prefetch: None,
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
            prefetch: None,
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
            prefetch: None,
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
            prefetch: None,
        });
        registry.add(RouteEntry {
            path: "/dashboard".to_string(),
            page: Some(dyn_page("<h1>Dash</h1>")),
            layout: Some(dyn_layout("dash")),
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
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
            prefetch: None,
        });
        registry.add(RouteEntry {
            path: "/dashboard".to_string(),
            page: None,
            layout: Some(dyn_layout("dash")),
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
        });
        registry.add(RouteEntry {
            path: "/dashboard/settings".to_string(),
            page: Some(dyn_page("settings-page")),
            layout: Some(dyn_layout("settings")),
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
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
            prefetch: None,
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
            prefetch: None,
        });
        registry.add(RouteEntry {
            path: "/about".to_string(),
            page: Some(dyn_page("about")),
            layout: None,
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
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
            prefetch: None,
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
            prefetch: None,
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
            prefetch: None,
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
            prefetch: None,
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
            prefetch: None,
        });
        registry.add(RouteEntry {
            path: "/dashboard".to_string(),
            page: Some(dyn_page("page")),
            layout: Some(dyn_layout("dash")),
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
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
            prefetch: None,
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
            prefetch: None,
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
            prefetch: None,
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
            prefetch: None,
        });
        registry.add(RouteEntry {
            path: "/dashboard".to_string(),
            page: Some(slow_dyn_page("<h1>D</h1>", 10)),
            layout: Some(dyn_layout("dash")),
            loading: Some(static_loading("loading-shell")),
            middleware: None,
            methods: vec![],
            prefetch: None,
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
            prefetch: None,
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
            prefetch: None,
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
            prefetch: None,
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
            prefetch: None,
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
            prefetch: None,
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
            prefetch: None,
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

    // -- not-found surfaces ---------------------------------------------------

    #[tokio::test]
    async fn test_not_found_renders_with_layout_and_404() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: Some(dyn_page("home")),
            layout: Some(dyn_layout("root")),
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
        });
        registry.add_not_found("/", static_page("nope"));

        let app = build_router(registry);

        // Matched route is unaffected.
        let ok = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::OK);

        // Unmatched path renders the not-found wrapped in the root layout.
        let nf = app
            .oneshot(
                Request::builder()
                    .uri("/does/not/exist")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(nf.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body_to_string(nf.into_body()).await,
            "<div class=\"root\">nope</div>",
        );
    }

    #[tokio::test]
    async fn test_deepest_not_found_wins() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: None,
            layout: Some(dyn_layout("root")),
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
        });
        registry.add(RouteEntry {
            path: "/admin".to_string(),
            page: None,
            layout: Some(dyn_layout("admin")),
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
        });
        registry.add_not_found("/", static_page("root-404"));
        registry.add_not_found("/admin", static_page("admin-404"));

        let app = build_router(registry);

        // Path under /admin picks the /admin surface, wrapped in root+admin layouts.
        let under_admin = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/missing")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(under_admin.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body_to_string(under_admin.into_body()).await,
            "<div class=\"root\"><div class=\"admin\">admin-404</div></div>",
        );

        // A path outside /admin falls to the root surface (root layout only).
        let elsewhere = app
            .oneshot(
                Request::builder()
                    .uri("/other")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(elsewhere.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body_to_string(elsewhere.into_body()).await,
            "<div class=\"root\">root-404</div>",
        );
    }

    #[tokio::test]
    async fn test_no_not_found_yields_bare_404() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: Some(dyn_page("home")),
            layout: None,
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(Request::builder().uri("/nope").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_to_string(resp.into_body()).await, "");
    }

    #[tokio::test]
    async fn test_not_found_surface_gets_speculation_in_head() {
        // A not-found wrapped in a layout with a <head> is a full document, so
        // (with speculation enabled) it gets the speculation-rules script just
        // like a normal page.
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: None,
            layout: Some(static_layout(
                "<html><head></head><body>{{children}}</body></html>",
            )),
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
        });
        registry.add_not_found("/", static_page("missing"));

        let app = build_router_with_speculation(registry, speculation_on());
        let resp = app
            .oneshot(Request::builder().uri("/nope").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = body_to_string(resp.into_body()).await;
        assert!(body.contains("speculationrules"));
        assert!(body.contains("missing"));
    }

    #[tokio::test]
    async fn test_public_dir_miss_falls_through_to_not_found() {
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
            prefetch: None,
        });
        registry.add_not_found("/", static_page("custom-404"));

        let app = build_router_with_public(registry, tmp.path());

        // Static file still served.
        let file = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/hello.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(file.status(), StatusCode::OK);
        assert_eq!(body_to_string(file.into_body()).await, "hi from public");

        // Missing file falls through ServeDir to the not-found surface.
        let missing = app
            .oneshot(
                Request::builder()
                    .uri("/missing.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_to_string(missing.into_body()).await, "custom-404");
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
            prefetch: None,
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
            prefetch: None,
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
            prefetch: None,
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

    // -- Speculation rules -----------------------------------------------------

    fn speculation_on() -> SpeculationConfig {
        SpeculationConfig {
            mode: crate::speculation::SpeculationMode::Prefetch,
            eagerness: crate::speculation::Eagerness::Moderate,
        }
    }

    #[tokio::test]
    async fn test_speculation_injected_into_full_document_head() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: Some(dyn_page("<h1>Home</h1>")),
            layout: Some(static_layout(
                "<html><head><title>t</title></head><body>{{children}}</body></html>",
            )),
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
        });

        let app = build_router_with_speculation(registry, speculation_on());
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = body_to_string(resp.into_body()).await;

        assert!(body.contains(r#"type="speculationrules""#));
        assert!(body.contains(r#""eagerness":"moderate""#));
        // Inside <head>, before the page content.
        let script = body.find("speculationrules").unwrap();
        let head_close = body.find("</head>").unwrap();
        let content = body.find("<h1>Home</h1>").unwrap();
        assert!(script < head_close);
        assert!(head_close < content);
    }

    #[tokio::test]
    async fn test_speculation_not_injected_into_headless_fragment() {
        // No <head> → a fragment; even with speculation enabled the output
        // stays byte-identical to speculation-off.
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: Some(dyn_page("<h1>Hi</h1>")),
            layout: Some(dyn_layout("root")),
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
        });

        let app = build_router_with_speculation(registry, speculation_on());
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = body_to_string(resp.into_body()).await;
        assert_eq!(body, "<div class=\"root\"><h1>Hi</h1></div>");
        assert!(!body.contains("speculationrules"));
    }

    #[tokio::test]
    async fn test_speculation_off_by_default() {
        // Behavior change in 0.4.0: build_router injects nothing unless the
        // app opts in via build_router_with_speculation.
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: Some(dyn_page("<h1>Home</h1>")),
            layout: Some(static_layout(
                "<html><head></head><body>{{children}}</body></html>",
            )),
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
        });

        let app = build_router(registry);
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = body_to_string(resp.into_body()).await;
        assert!(!body.contains("speculationrules"));
        assert_eq!(body, "<html><head></head><body><h1>Home</h1></body></html>");
    }

    // The pre-0.4.0 names keep compiling for one release. Deprecation warnings
    // here are expected and deliberately not silenced.
    #[tokio::test]
    async fn test_deprecated_router_names_still_compile() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: Some(dyn_page("<h1>Home</h1>")),
            layout: Some(static_layout(
                "<html><head></head><body>{{children}}</body></html>",
            )),
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
        });

        let app = build_router_with_prefetch(registry, crate::PrefetchConfig::OFF);
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = body_to_string(resp.into_body()).await;
        assert!(!body.contains("speculationrules"));
    }

    #[tokio::test]
    async fn test_speculation_rules_exclude_react_app_shell_routes() {
        // Mixed app: server-rendered /docs pages plus React app-shell routes
        // at / and /source/{id}. With speculation enabled, the injected rules
        // must exclude the React URLs (the shell soft-navigates them; a
        // prefetched document would be discarded) while still covering the
        // server routes via the broad match.
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/docs".to_string(),
            page: Some(dyn_page("<h1>Docs</h1>")),
            layout: Some(static_layout(
                "<html><head></head><body>{{children}}</body></html>",
            )),
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
        });
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: Some(dyn_page("react shell")),
            layout: None,
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
        });
        registry.add(RouteEntry {
            path: "/source/{id}".to_string(),
            page: Some(dyn_page("react shell")),
            layout: None,
            loading: None,
            middleware: None,
            methods: vec![],
            prefetch: None,
        });
        registry.mark_react_page("/");
        registry.mark_react_page("/source/{id}");

        let app = build_router_with_speculation(registry, speculation_on());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/docs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = body_to_string(resp.into_body()).await;

        // Rules present (server page), broad same-origin match intact...
        assert!(body.contains(r#"type="speculationrules""#));
        assert!(body.contains(r#""href_matches":"/*""#));
        // ...but the React routes are excluded, with the dynamic segment
        // converted to URL Pattern syntax.
        assert!(
            body.contains(r#""not":{"href_matches":["/","/source/:id"]}"#),
            "expected React-route exclusion clause in:\n{body}"
        );
    }

    #[tokio::test]
    async fn test_speculation_injected_in_streaming_path() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/dashboard".to_string(),
            page: Some(slow_dyn_page("<h1>D</h1>", 10)),
            layout: Some(static_layout(
                "<html><head></head><body>{{children}}</body></html>",
            )),
            loading: Some(static_loading("loading")),
            middleware: None,
            methods: vec![],
            prefetch: None,
        });

        let app = build_router_with_speculation(registry, speculation_on());
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
        // Injected into the streamed head, before the loading slot.
        let script = body.find("speculationrules").expect("script present");
        let slot = body.find("__nx_slot__").expect("slot present");
        assert!(script < slot);
    }

    fn prefetch_entry(path: &str, prefetch: crate::conventions::PrefetchDataFn) -> RouteEntry {
        RouteEntry {
            path: path.to_string(),
            page: Some(dyn_page("shell")),
            prefetch: Some(prefetch),
            ..Default::default()
        }
    }

    fn one_seed(url: &'static str, data: serde_json::Value) -> crate::conventions::PrefetchDataFn {
        Box::new(move |_req| {
            let data = data.clone();
            Box::pin(async move {
                crate::seed::QuerySeed::new()
                    .seed(async move {
                        crate::seed::SeedEntry {
                            key: crate::seed::seed_key(url, None),
                            data,
                        }
                    })
                    .await
            })
        })
    }

    #[tokio::test]
    async fn prefetch_endpoint_serves_a_static_routes_seeds() {
        let mut registry = RouteRegistry::new();
        registry.add(prefetch_entry(
            "/todos",
            one_seed("/api/todos", serde_json::json!([1, 2])),
        ));
        let app = build_router(registry);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/__nx/prefetch?path=%2Ftodos")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_to_string(resp.into_body()).await;
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed[0]["key"], serde_json::json!(["/api/todos"]));
        assert_eq!(parsed[0]["data"], serde_json::json!([1, 2]));
    }

    #[tokio::test]
    async fn prefetch_endpoint_extracts_dynamic_params_and_query() {
        let mut registry = RouteRegistry::new();
        registry.add(prefetch_entry(
            "/todos/{id}",
            Box::new(|req| {
                Box::pin(async move {
                    let (params, req) = crate::params::extract_params(req).await;
                    let id = params.get("id").unwrap_or("?").to_string();
                    let q = req.uri().query().unwrap_or("").to_string();
                    crate::seed::QuerySeed::new()
                        .seed(async move {
                            crate::seed::SeedEntry {
                                key: serde_json::json!([format!("/api/todos/{id}")]),
                                data: serde_json::json!({ "query": q }),
                            }
                        })
                        .await
                })
            }),
        ));
        let app = build_router(registry);

        // path carries params AND a query string (prefetch.rs may read both).
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/__nx/prefetch?path=%2Ftodos%2F7%3Fstatus%3Dopen")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let parsed: serde_json::Value =
            serde_json::from_str(&body_to_string(resp.into_body()).await).unwrap();
        assert_eq!(parsed[0]["key"], serde_json::json!(["/api/todos/7"]));
        assert_eq!(parsed[0]["data"]["query"], serde_json::json!("status=open"));
    }

    #[tokio::test]
    async fn prefetch_endpoint_runs_the_routes_middleware() {
        // A protected page's data must stay protected — the middleware chain
        // short-circuits the prefetch exactly like it would the page.
        let mut registry = RouteRegistry::new();
        let mut entry = prefetch_entry("/admin", one_seed("/api/secrets", serde_json::json!(1)));
        entry.middleware = Some(Box::new(|_req| {
            Box::pin(async {
                crate::conventions::MiddlewareResult::response(StatusCode::UNAUTHORIZED)
            })
        }));
        registry.add(entry);
        let app = build_router(registry);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/__nx/prefetch?path=%2Fadmin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn prefetch_endpoint_rejects_bad_targets() {
        let mut registry = RouteRegistry::new();
        registry.add(prefetch_entry(
            "/todos",
            one_seed("/api/todos", serde_json::json!(1)),
        ));
        let app = build_router(registry);

        for (uri, expect) in [
            ("/__nx/prefetch", StatusCode::BAD_REQUEST),
            (
                "/__nx/prefetch?path=https%3A%2F%2Fevil.example",
                StatusCode::BAD_REQUEST,
            ),
            (
                "/__nx/prefetch?path=%2F%2Fevil.example",
                StatusCode::BAD_REQUEST,
            ),
            // A real path with no prefetch-capable route: plain 404.
            ("/__nx/prefetch?path=%2Fnope", StatusCode::NOT_FOUND),
        ] {
            let resp = app
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(resp.status(), expect, "{uri}");
        }
    }

    #[tokio::test]
    async fn prefetch_endpoint_absent_without_prefetch_routes() {
        let mut registry = RouteRegistry::new();
        registry.add(RouteEntry {
            path: "/".to_string(),
            page: Some(dyn_page("home")),
            ..Default::default()
        });
        let app = build_router(registry);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/__nx/prefetch?path=%2F")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
