use axum::body::Body;
use axum::handler::Handler;
use axum::response::{IntoResponse, Response};
use std::future::Future;
use std::pin::Pin;

/// A rendered HTML string (the output of an Askama template `.render()`)
pub type HtmlString = String;

/// Async function that produces page HTML (the main content)
pub type PageFn = Box<
    dyn Fn(http::Request<axum::body::Body>) -> Pin<Box<dyn Future<Output = HtmlString> + Send>>
        + Send
        + Sync,
>;

/// Sync function that wraps child HTML in a layout
/// Takes child HTML, returns the full wrapped HTML
pub type LayoutFn = Box<dyn Fn(&str) -> HtmlString + Send + Sync>;

/// Sync function that returns loading skeleton HTML.
/// The framework handles the loading→page swap; the loading content is just
/// what the user sees while the page is being computed.
pub type LoadingFn = Box<dyn Fn() -> HtmlString + Send + Sync>;

/// Result from `middleware.rs::handle`.
///
/// Middleware runs before page rendering, loading-shell streaming, and
/// `route.rs` method handlers. It can either continue with the request, or
/// stop routing and return a response while status/headers are still mutable.
pub enum MiddlewareResult {
    Continue(http::Request<Body>),
    Response(Response),
}

impl MiddlewareResult {
    pub fn next(req: http::Request<Body>) -> Self {
        Self::Continue(req)
    }

    pub fn response(response: impl IntoResponse) -> Self {
        Self::Response(response.into_response())
    }
}

/// Async request middleware from `middleware.rs`.
pub type MiddlewareFn = Box<
    dyn Fn(http::Request<Body>) -> Pin<Box<dyn Future<Output = MiddlewareResult> + Send>>
        + Send
        + Sync,
>;

/// Async handler for API routes (route.rs)
pub type RouteFn = Box<
    dyn Fn(http::Request<axum::body::Body>) -> Pin<Box<dyn Future<Output = Response> + Send>>
        + Send
        + Sync,
>;

/// Data prefetch for a route's React page (from `prefetch.rs`/`props.rs`):
/// runs the same server logic a hard load streams as `__nx_seeds__`, so the
/// soft-nav prefetch endpoint (`/__nx/prefetch?path=...`) can warm the client
/// cache during navigation. Params (on dynamic routes) are extracted from the
/// request inside the generated closure, exactly like the page handler does.
pub type PrefetchDataFn = Box<
    dyn Fn(
            http::Request<axum::body::Body>,
        ) -> Pin<Box<dyn Future<Output = crate::seed::QuerySeed> + Send>>
        + Send
        + Sync,
>;

/// Represents a single route entry discovered from the app/ directory
#[derive(Default)]
pub struct RouteEntry {
    /// URL path, e.g. "/" or "/dashboard/settings"
    pub path: String,
    /// The page handler (from page.rs or page.html)
    pub page: Option<PageFn>,
    /// Layout wrapper (from layout.rs or layout.html) — applies to this segment and children
    pub layout: Option<LayoutFn>,
    /// Loading skeleton (from loading.rs or loading.html)
    pub loading: Option<LoadingFn>,
    /// Request middleware (from middleware.rs)
    pub middleware: Option<MiddlewareFn>,
    /// API route handlers by method (from route.rs)
    pub methods: Vec<(http::Method, RouteFn)>,
    /// Data prefetch (from prefetch.rs/props.rs beside a page.tsx) — served
    /// by the soft-nav prefetch endpoint so soft navigations render seeded
    /// like hard loads.
    pub prefetch: Option<PrefetchDataFn>,
}

/// A `not-found.{rs,html,tsx}` surface, keyed by the URL path of the segment
/// that declared it. When no route matches a request, the router renders the
/// entry whose `path` is the *deepest* ancestor of the requested path, wrapped
/// in that segment's layouts, with a `404` status. Mirrors Next.js's
/// `not-found.tsx`, which is scoped to its segment subtree.
pub struct NotFoundEntry {
    /// URL path of the declaring segment, e.g. "/" or "/admin".
    pub path: String,
    /// Renders the 404 body (without layouts — the router applies those). Same
    /// [`PageFn`] shape as a page, so the three variants reuse the page helpers:
    /// `not-found.rs` calls its `render(req)`, `not-found.html` is wrapped with
    /// [`static_page`], and `not-found.tsx` becomes a client-rendered shell.
    pub render: PageFn,
}

/// A collection of route entries that gets turned into an Axum router
pub struct RouteRegistry {
    pub entries: Vec<RouteEntry>,
    /// Subtree-scoped 404 surfaces, installed as the router's fallback.
    pub not_found: Vec<NotFoundEntry>,
}

impl RouteRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            not_found: Vec::new(),
        }
    }

    pub fn add(&mut self, entry: RouteEntry) {
        self.entries.push(entry);
    }

    /// Register a `not-found` surface for the subtree rooted at `path`.
    pub fn add_not_found(&mut self, path: impl Into<String>, render: PageFn) {
        self.not_found.push(NotFoundEntry {
            path: path.into(),
            render,
        });
    }
}

// -- Static helpers -----------------------------------------------------------
// Wrap raw HTML strings (typically from `.html` convention files via
// `include_str!`) as the appropriate handler types. Codegen for `.html` files
// uses these so the rest of the framework only deals with closures.

/// Wrap a static HTML string as a [`PageFn`].
pub fn static_page(html: &'static str) -> PageFn {
    Box::new(move |_req| Box::pin(async move { html.to_string() }))
}

/// Wrap a static layout template as a [`LayoutFn`]. The template must contain
/// `{{ children }}` (askama-compatible; surrounding whitespace optional),
/// which is replaced with the rendered child content at request time.
pub fn static_layout(template: &'static str) -> LayoutFn {
    Box::new(move |children| {
        template
            .replace("{{ children }}", children)
            .replace("{{children}}", children)
    })
}

/// Wrap a static HTML string as a [`LoadingFn`].
pub fn static_loading(html: &'static str) -> LoadingFn {
    Box::new(move || html.to_string())
}

/// Wrap an exported `route.rs` method function as a [`RouteFn`].
///
/// Accepts any Axum [`Handler`] — that includes both the "raw" form that takes
/// the whole request:
///
/// ```ignore
/// pub async fn post(req: Request<Body>) -> impl IntoResponse { ... }
/// ```
///
/// and the typed form using Axum extractors with a concrete return type, which
/// is what the OpenAPI codegen wants so the request/response shapes are
/// recoverable:
///
/// ```ignore
/// #[utoipa::path(post, path = "/api/ping", request_body = PingRequest,
///                responses((status = 200, body = PingResponse)))]
/// pub async fn post(Json(body): Json<PingRequest>) -> Json<PingResponse> { ... }
/// ```
///
/// Handlers here are stateless, so the handler's state type is `()`. The
/// concrete handler is converted to its boxed future at call time via
/// [`Handler::call`]; the framework's router supplies the (possibly
/// middleware-mutated) request.
pub fn route_method<H, T>(handler: H) -> RouteFn
where
    H: Handler<T, ()> + Sync,
    T: 'static,
{
    Box::new(move |req| {
        let handler = handler.clone();
        Box::pin(async move { handler.call(req, ()).await })
            as Pin<Box<dyn Future<Output = Response> + Send>>
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_static_page_returns_html_verbatim() {
        let p = static_page("<h1>Hello</h1>");
        let req = http::Request::builder()
            .body(axum::body::Body::empty())
            .unwrap();
        let html = p(req).await;
        assert_eq!(html, "<h1>Hello</h1>");
    }

    #[test]
    fn test_static_layout_substitutes_children() {
        let l = static_layout("<html><body>{{children}}</body></html>");
        let out = l("<h1>Hi</h1>");
        assert_eq!(out, "<html><body><h1>Hi</h1></body></html>");
    }

    #[test]
    fn test_static_layout_supports_askama_whitespace_form() {
        let l = static_layout("<html>{{ children }}</html>");
        let out = l("X");
        assert_eq!(out, "<html>X</html>");
    }

    #[test]
    fn test_static_layout_with_repeated_children_marker() {
        // Multiple {{children}} in a template all get replaced — useful for
        // when the same content needs to appear in multiple slots, but mostly
        // documenting current behavior so it doesn't drift accidentally.
        let l = static_layout("<a>{{children}}</a><b>{{children}}</b>");
        let out = l("X");
        assert_eq!(out, "<a>X</a><b>X</b>");
    }

    #[test]
    fn test_static_loading_returns_html_verbatim() {
        let l = static_loading("<div>Loading...</div>");
        assert_eq!(l(), "<div>Loading...</div>");
    }

    #[tokio::test]
    async fn test_add_not_found_records_entry_and_render() {
        let mut registry = RouteRegistry::new();
        assert!(registry.not_found.is_empty());

        registry.add_not_found("/admin", static_page("<h1>404</h1>"));

        assert_eq!(registry.not_found.len(), 1);
        let entry = &registry.not_found[0];
        assert_eq!(entry.path, "/admin");

        let req = http::Request::builder()
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!((entry.render)(req).await, "<h1>404</h1>");
    }

    #[tokio::test]
    async fn test_middleware_result_helpers() {
        let req = http::Request::builder()
            .body(axum::body::Body::empty())
            .unwrap();
        match MiddlewareResult::next(req) {
            MiddlewareResult::Continue(_) => {}
            MiddlewareResult::Response(_) => panic!("expected continue"),
        }

        match MiddlewareResult::response(axum::http::StatusCode::UNAUTHORIZED) {
            MiddlewareResult::Continue(_) => panic!("expected response"),
            MiddlewareResult::Response(resp) => {
                assert_eq!(resp.status(), axum::http::StatusCode::UNAUTHORIZED);
            }
        }
    }

    #[tokio::test]
    async fn test_route_method_wraps_into_response() {
        async fn handler(_req: http::Request<axum::body::Body>) -> impl IntoResponse {
            axum::http::StatusCode::CREATED
        }

        let route = route_method(handler);
        let req = http::Request::builder()
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = route(req).await;

        assert_eq!(resp.status(), axum::http::StatusCode::CREATED);
    }
}
