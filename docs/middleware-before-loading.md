# Middleware Before Loading: Auth, Redirects, And Streaming

Status: implemented in `nextrs` with `middleware.rs::handle(req) -> MiddlewareResult`.

## Context

`nextrs` currently has two separate handler models:

- `page.rs` owns UI `GET` rendering and unlocks `loading.{rs,html}` streaming.
- `route.rs` owns HTTP method handlers and can return arbitrary Axum responses.

That split is fine for simple pages, but it creates a problem for authenticated pages. A protected route such as `/reviews` wants both:

1. A real HTTP redirect before rendering when the user is not authenticated.
2. A streamed loading shell while slow page data is fetched after auth succeeds.

Today, `page.rs` returns only `String` HTML:

```rust
pub async fn render(req: http::Request<axum::body::Body>) -> String
```

Once `nextrs` starts streaming the loading shell, the response status and headers are already committed. At that point the page can no longer cleanly return `303 See Other`, `401 Unauthorized`, or a custom error response.

This is the same class of problem Next.js solves with middleware: auth/canonical-host/tenant routing runs before the route tree and before `loading.tsx` can stream.

## Goal

Add a framework-level mechanism that lets an app run request guards before page rendering and before loading-shell streaming.

The primary use case is:

```text
request /reviews
  -> middleware checks canonical host
  -> middleware checks auth session
  -> redirect if needed, before any response body is sent
  -> otherwise enter nextrs page rendering
  -> stream loading shell immediately
  -> await page data
  -> swap in final page content
```

## Non-Goals

- Do not make `loading` handle auth. Loading states should only represent allowed page work.
- Do not fake redirects inside HTML with JavaScript.
- Do not require authenticated pages to be written as `route.rs`; that bypasses the `page/loading/layout` model.
- Do not introduce a client-side runtime.
- Do not solve nested Suspense-style streaming in this work.

## Recommended Design

Implement a middleware convention that runs before `router.rs::render_route`.

Suggested convention files:

```text
app/middleware.rs                applies globally
app/reviews/middleware.rs        applies to /reviews and nested routes
app/settings/middleware.rs       applies to /settings and nested routes
```

Middleware should be able to either continue or return a response:

```rust
pub enum MiddlewareResult {
    Continue(http::Request<axum::body::Body>),
    Response(axum::response::Response),
}
```

The simplest user-facing API can be:

```rust
pub async fn handle(
    req: http::Request<axum::body::Body>,
) -> nextrs::conventions::MiddlewareResult {
    if !is_logged_in(&req) {
        return nextrs::conventions::MiddlewareResult::Response(
            axum::response::Redirect::to("/auth/login").into_response(),
        );
    }

    nextrs::conventions::MiddlewareResult::Continue(req)
}
```

For ergonomics, add helper constructors:

```rust
impl MiddlewareResult {
    pub fn next(req: Request<Body>) -> Self;
    pub fn response(response: impl IntoResponse) -> Self;
}
```

## Why Middleware Instead Of PageResult First

A page-result type is useful, but it does not fully solve redirects with streaming.

For example:

```rust
pub enum PageResult {
    Html(String),
    Response(axum::response::Response),
}
```

This lets a page return redirects/errors, but the router has to await the page before deciding whether it can stream the loading shell. That weakens the main UX benefit because slow DB work inside the page delays the loading state.

Middleware preserves the intended split:

- Middleware does quick request gating and may return real HTTP responses.
- Page rendering does slower data work and can show `loading` while it runs.

`PageResult` can still be added later for page-local not-found/error responses, but auth redirects should happen before loading.

## Route Matching Semantics

Middleware should compose from root to leaf, like layouts:

```text
app/middleware.rs
app/dashboard/middleware.rs
app/dashboard/settings/middleware.rs
```

A request to `/dashboard/settings` should run them in this order:

1. `/` middleware
2. `/dashboard` middleware
3. `/dashboard/settings` middleware

If any middleware returns `Response`, stop immediately and return it. Do not render layouts, loading, or page content.

If all middleware returns `Continue(req)`, pass the final request to the page/route handler.

This should apply to both page routes and `route.rs` method handlers. Auth usually protects API routes too.

## Discovery Changes

Update `nextrs/src/discovery.rs` to detect:

```text
middleware.rs
```

Add it to `DiscoveredRoute`, parallel to `layout`, `loading`, and `route`.

Suggested field:

```rust
pub middleware: Option<PathBuf>
```

This is `.rs` only for now. There is no useful static HTML middleware variant.

## Convention Types

Update `nextrs/src/conventions.rs`:

```rust
pub enum MiddlewareResult {
    Continue(http::Request<axum::body::Body>),
    Response(axum::response::Response),
}

pub type MiddlewareFn = Box<
    dyn Fn(http::Request<axum::body::Body>)
        -> Pin<Box<dyn Future<Output = MiddlewareResult> + Send>>
        + Send
        + Sync,
>;

pub struct RouteEntry {
    pub path: String,
    pub page: Option<PageFn>,
    pub layout: Option<LayoutFn>,
    pub loading: Option<LoadingFn>,
    pub middleware: Option<MiddlewareFn>,
    pub methods: Vec<(http::Method, RouteFn)>,
}
```

Add helper methods on `MiddlewareResult` so app code does not need to import enum variants everywhere.

## Codegen Changes

Update `nextrs/src/build.rs`:

1. Emit `#[path = "..."] mod __nextrs_route_N_middleware;` for discovered middleware files.
2. Emit the `middleware` field in every `RouteEntry`.
3. Wrap `middleware.rs::handle(req)` in the boxed function type.

Generated shape:

```rust
middleware: Some(Box::new(|req| Box::pin(__nextrs_route_3_middleware::handle(req)))),
```

Routes without middleware should emit:

```rust
middleware: None,
```

Update codegen tests to assert:

- middleware modules are emitted
- route entries include `middleware: Some(...)`
- `.rs` page/layout/loading behavior is unchanged
- `route.rs` method generation is unchanged

## Router Changes

Update `nextrs/src/router.rs`.

Add a collector similar to layout collection:

```rust
fn collect_middlewares_for_path<'a>(
    entries: &'a [RouteEntry],
    target_path: &str,
) -> Vec<&'a MiddlewareFn>
```

Ordering should be root to leaf, matching layout ordering.

Before rendering a page with or without loading:

```rust
let mut req = req;
for middleware in collect_middlewares_for_path(&entries, &path) {
    match middleware(req).await {
        MiddlewareResult::Continue(next_req) => req = next_req,
        MiddlewareResult::Response(response) => return response,
    }
}

render_route_after_middleware(..., req).await
```

Important: run middleware before calling `layout_shell` and before yielding any streaming chunks. That preserves redirects/status codes.

Also run middleware before `route.rs` method handlers. The current method routing directly invokes `route_fn(req).await`; wrap it with the same middleware pipeline.

## Request Body Considerations

Middleware takes ownership of `Request<Body>`.

For auth/canonical-host middleware, it should inspect headers/cookies/URI and return the request unchanged. Avoid reading the body in middleware unless there is a clear design for reconstructing it.

Document this constraint:

- Middleware may read request metadata freely.
- Middleware should not consume the body unless it replaces it before returning `Continue`.

## Example Auth Middleware

Example app file:

```rust
use axum::response::{IntoResponse, Redirect};
use http::header;
use nextrs::conventions::MiddlewareResult;

pub async fn handle(
    req: http::Request<axum::body::Body>,
) -> MiddlewareResult {
    let path = req
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");

    let has_session = req
        .headers()
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|cookie| cookie.contains("session="));

    if !has_session {
        return MiddlewareResult::response(
            Redirect::to(&format!("/auth/login?return_to={}", urlencoding::encode(path)))
                .into_response(),
        );
    }

    MiddlewareResult::next(req)
}
```

This would let a route like:

```text
app/reviews/
  middleware.rs
  page.rs
  loading.html
```

have real redirects and still stream loading after auth succeeds.

## Tests To Add

Add router tests:

1. **Middleware redirect prevents loading stream**
   - Route has page + loading + middleware.
   - Middleware returns `Redirect`.
   - Assert response status is `303`.
   - Assert body does not contain loading shell.

2. **Middleware continue preserves loading stream**
   - Middleware returns `Continue`.
   - Page sleeps.
   - Assert loading frame arrives before page resolves, using the existing streaming timing pattern.

3. **Nested middleware order is root to leaf**
   - Root middleware adds a header or request extension.
   - Child middleware observes it or records order.
   - Assert order.

4. **Middleware applies to route.rs methods**
   - POST route has middleware.
   - Middleware returns `401`.
   - Assert POST handler is not called.

5. **Middleware can modify request**
   - Middleware inserts an extension.
   - Page or route handler reads it.
   - Assert value is visible.

Add discovery/codegen tests:

1. `middleware.rs` is discovered.
2. Generated registry includes middleware field.
3. Existing routes without middleware emit `middleware: None`.

## Pulltime Acceptance Scenario

Pulltime should be able to move `/reviews` from `route.rs` to:

```text
app/reviews/
  middleware.rs
  page.rs
  loading.rs
  loading.html
```

Expected behavior:

1. Unauthenticated request to `/reviews` returns a real HTTP redirect to GitHub login.
2. Wrong-host request returns a real canonical-host redirect.
3. Authenticated request to `/reviews` streams loading shell before DB queue queries finish.
4. Final page swaps into the existing root layout.
5. No JavaScript redirect hacks are needed.

Verification commands:

```bash
curl -i http://localhost:3000/reviews
# unauthenticated: HTTP 303, Location: /auth/...

curl --no-buffer --trace-time --trace - http://localhost:3000/reviews 2>&1 \
  | grep "<= Recv data"
# authenticated/dev session: multiple chunks when loading is present
```

## Open Questions

1. Should middleware be path-segment convention files only, or should `build_router` also accept global middleware programmatically?
2. Should middleware apply to static public files? Recommendation: no. Static files are handled by `ServeDir` fallback or CDN before the app route.
3. Should middleware be able to return typed errors that route into `error.{rs,html}` later? Recommendation: not in this first pass.
4. Should `PageResult` still be added? Recommendation: yes later, for page-local not-found/error responses, but not as the auth solution.

## Suggested Implementation Order

1. Add `MiddlewareResult` and `MiddlewareFn` to `conventions.rs`.
2. Add `middleware` field to `RouteEntry`.
3. Update all tests/builders that construct `RouteEntry` manually.
4. Add discovery support for `middleware.rs`.
5. Add codegen support and tests.
6. Add router middleware collection and execution before page streaming.
7. Add method-handler middleware execution.
8. Add demo route under `site/app/with-middleware-loading/`.
9. Update `README.md`, `MANIFEST.md`, and `docs/streaming.md`.
10. Validate with `cargo test --workspace --all-features` and a local curl streaming check.
