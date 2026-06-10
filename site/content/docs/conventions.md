+++
title = "Routing Conventions"
description = "page, layout, loading, middleware, and route files — what each does and how they compose"
section = "Guides"
order = 2
+++

Every directory under `app/` is a URL segment. Five file names have meaning inside a segment:

| File | Role | Signature |
|---|---|---|
| `page.{rs,html}` | The content for this URL | `pub async fn render(Request<Body>) -> String` |
| `layout.{rs,html}` | Wraps this segment's children (and nested routes) | `pub fn render(children: &str) -> String` |
| `loading.{rs,html}` | Skeleton streamed while the page computes | `pub fn render() -> String` |
| `middleware.rs` | Guard that runs before anything renders | `pub async fn handle(Request<Body>) -> MiddlewareResult` |
| `route.rs` | API handlers (JSON etc.) | `pub async fn get/post/put/patch/delete/...` |

For `page`, `layout`, and `loading`, both `.rs` and `.html` are accepted; if both exist, **`.rs` wins**. An `.html` file is served as-is (for layouts, `{{ children }}` is substituted literally) — zero Rust required for static segments.

## Pages

```rust
use askama::Template;

#[derive(Template)]
#[template(path = "users/page.html")]
pub struct UsersPage { pub names: Vec<String> }

pub async fn render(req: http::Request<axum::body::Body>) -> String {
    let names = fetch_users().await;
    UsersPage { names }.render().unwrap()
}
```

Pages receive the full request: headers, URI, and any extensions middleware inserted. They return the rendered HTML string; the framework wraps it in the layout chain and the HTTP response.

## Layouts

Layouts nest: a request to `/a/b` renders `app/layout` around `app/a/layout` around `app/a/b/page`, root to leaf.

```rust
use askama::Template;

#[derive(Template)]
#[template(path = "layout.html")]
pub struct RootLayout<'a> { pub children: &'a str }

pub fn render(children: &str) -> String {
    RootLayout { children }.render().unwrap()
}
```

**Askama layouts must use `{{ children|safe }}`.** Without `|safe`, Askama HTML-escapes the children — which breaks both your page markup and the framework's internal content marker (see [Streaming](/docs/streaming) for why that marker exists). This is the most common first-run mistake.

## Loading

A `loading.{rs,html}` file opts the route into streaming: the loading skeleton is sent immediately, the page handler runs concurrently, and the resolved page is swapped in on the same response. Routes without a loading slot return one synchronous response. Details in [Streaming](/docs/streaming).

## Middleware

`middleware.rs` files compose root-to-leaf along the matched path and run **before** layouts, loading, pages, and API handlers:

```rust
use axum::body::Body;
use http::Request;
use nextrs::conventions::MiddlewareResult;

pub async fn handle(mut req: Request<Body>) -> MiddlewareResult {
    let Some(user) = authenticate(&req).await else {
        return MiddlewareResult::response((
            http::StatusCode::SEE_OTHER,
            [("location", "/login")],
        ));
    };
    req.extensions_mut().insert(user);
    MiddlewareResult::next(req)
}
```

`MiddlewareResult::next(req)` continues (pass the request along — you may have mutated it); `MiddlewareResult::response(...)` short-circuits with a real HTTP response. Because middleware runs before the loading shell is sent, redirects and auth failures get correct status codes and headers even on streaming routes. Downstream pages read what middleware inserted via `req.extensions().get::<User>()`.

## API routes

`route.rs` exports one public async function per HTTP method. Handlers are ordinary Axum handlers — extractors in, `impl IntoResponse` out:

```rust
use axum::Json;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct Pong { pub message: String }

pub async fn get() -> Json<Pong> {
    Json(Pong { message: "pong".into() })
}
```

The build step detects which methods a `route.rs` exports by name. A segment can have both `page.rs` and `route.rs` — the page owns GET, the route file handles the rest. **Exporting `get()` from a `route.rs` next to a page is a compile error** (the build emits `compile_error!` with the conflicting path), so the conflict can't ship.

To generate a typesafe TypeScript client from your `route.rs` handlers, see [Typesafe Client Generation](/docs/typesafe-client).

## Dynamic segments

A directory named `[param]` matches one path segment:

```
app/users/[id]/page.rs   →  /users/{id}
```

Inside the handler, extract the parameter with Axum's `Path` extractor:

```rust
use axum::extract::Path;
use axum::RequestPartsExt;

pub async fn render(req: http::Request<axum::body::Body>) -> String {
    let (mut parts, _body) = req.into_parts();
    let Path(id): Path<String> = parts.extract().await.unwrap();
    format!("<h1>user {}</h1>", id)
}
```

## Static assets

Files in `public/` (sibling of `app/`) are served at the root URL path: `public/style.css` → `/style.css`. Locally they're a router fallback (routes win over files); on Vercel the CDN serves them before the function is invoked (files win over routes). Don't give a route and a file the same name and the asymmetry never matters.
