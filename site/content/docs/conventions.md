+++
title = "Routing Conventions"
description = "Exact route filenames, free colocation, dynamic URLs, APIs, and server prefetch"
section = "Guides"
order = 2
+++

Directories under `app/` describe URL segments. Files only acquire framework
meaning when their names match a convention exactly; every other `.ts`, `.tsx`,
or Rust module is ordinary colocated application code.

## Recognized files

| File | Role |
|---|---|
| `page.tsx` | React page for this URL |
| `layout.tsx` | React layout for this segment and descendants |
| `loading.tsx` | React pending UI |
| `not-found.tsx` | React 404 surface for this subtree |
| `middleware.rs` | Request guard or transformation |
| `route.rs` | Axum handlers named for HTTP methods |
| `prefetch.rs` | Server data that warms a sibling `page.tsx` query cache |

New applications should use the React conventions above. `prefetch.rs`
requires a `page.tsx` sibling because it feeds that React page's cache.

<details>
<summary>Legacy server-rendered conventions</summary>

Earlier nextrs applications may contain `page.rs`/`page.html`,
`layout.rs`/`layout.html`, `loading.rs`/`loading.html`, or
`not-found.rs`/`not-found.html`. They remain compatibility conventions for
existing applications, but are not part of the recommended React-first model
for new projects. A legacy rendering slot cannot coexist with its `.tsx` form
in the same segment.

</details>

## Colocation is free

The router ignores files outside the table. Both of these are valid:

```text
app/todos/page.tsx
app/todos/TodoRow.tsx          # used only by /todos
app/todos/format-todo.ts       # ordinary helper
components/Button.tsx          # shared by many routes
```

You do not need an underscore-prefixed component directory. Keep route-local
code near its page; use top-level `components/` for broadly reusable React UI.
Application Rust and domain logic normally live in `src/`, while `route.rs`,
`middleware.rs`, and `prefetch.rs` stay thin web adapters.

## React pages and layouts

```tsx
// app/users/page.tsx -> /users
export default function UsersPage() {
  return <h1>Users</h1>;
}
```

Layouts nest from root to leaf and receive the matched page as `children`:

```tsx
import type { ReactNode } from "react";

export default function DashboardLayout({ children }: { children: ReactNode }) {
  return <section><aside>Dashboard</aside><main>{children}</main></section>;
}
```

`app/layout.tsx` wraps all React routes.
`app/dashboard/layout.tsx` adds a layer only below `/dashboard`.

## Loading and server prefetch

Place `loading.tsx` beside or above a React page to define its pending UI:

```tsx
export default function Loading() {
  return <p>Loading…</p>;
}
```

A `prefetch.rs` beside `page.tsx` returns a `nextrs::QuerySeed`. On a hard
load, nextrs includes those cache entries in the page shell. On link intent
and soft navigation, the app shell warms the route chunk and data. See
[React Pages & Server Prefetch](/docs/react-server-props).

## Middleware

`middleware.rs` files compose from root to leaf and run before pages and API
handlers:

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

## Typed API routes

`route.rs` exports async functions named `get`, `post`, `put`, `patch`,
`delete`, `head`, or `options`. Axum extractors define the inputs and concrete
response types define the output:

```rust
use axum::{extract::Path, Json};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct User { pub id: u64 }

#[nextrs::api]
pub async fn get(Path(id): Path<u64>) -> Json<User> {
    Json(User { id })
}
```

The handler routes without the annotation. `#[nextrs::api]` additionally puts
it in the generated OpenAPI/client contract. See
[Client Generation: Step by Step](/docs/client-codegen).

## Dynamic and catch-all segments

```text
app/users/[id]/page.tsx          -> /users/{id}
app/api/users/[id]/route.rs      -> /api/users/{id}
app/api/auth/[...all]/route.rs   -> /api/auth/*all
```

A React page receives dynamic values through its typed `params` prop. API
handlers use Axum's `Path<T>` extractor. The generated client carries typed
path arguments rather than asking callers to assemble URLs.

For multiple path parameters, use one `Path` extractor with a named struct.
The field names match the dynamic directory names:

```text title="Route"
app/api/organizations/[organization_id]/todos/[todo_id]/route.rs
```

```rust title="app/api/organizations/[organization_id]/todos/[todo_id]/route.rs"
use axum::{extract::Path, Json};
use serde::Deserialize;
use utoipa::IntoParams;

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Path)]
pub struct TodoPath {
    pub organization_id: u64,
    pub todo_id: u64,
}

#[nextrs::api]
pub async fn get(Path(path): Path<TodoPath>) -> Json<Todo> {
    find_todo(path.organization_id, path.todo_id).await
}
```

A tuple is supported as a compact alternative. Its values follow the URL
segment order:

```rust title="Tuple shorthand"
#[nextrs::api]
pub async fn get(
    Path((organization_id, todo_id)): Path<(u64, u64)>,
) -> Json<Todo> {
    find_todo(organization_id, todo_id).await
}
```

Use a single scalar such as `Path<u64>` only when the route has one dynamic
segment. Invalid scalar, tuple, or multiple-`Path` shapes produce a compiler
error that recommends the named-struct or tuple form.

## Handler arguments are extractors

A `route.rs` function is an ordinary Axum handler. Each argument is an
**extractor** that tells Axum where its value comes from. Axum performs the
runtime extraction; `#[nextrs::api]` inspects the request-contract types to
describe the endpoint in OpenAPI and generate its TypeScript client.

```rust title="app/api/organizations/[organization_id]/todos/[todo_id]/route.rs"
use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::HeaderMap,
};
use nextrs::{ApiError, Timing};

#[nextrs::api]
pub async fn get(
    Path(path): Path<TodoPath>,
    Query(query): Query<TodoQuery>,
    Extension(ctx): Extension<AppContext>,
    headers: HeaderMap,
    timing: Timing,
) -> Result<Json<Todo>, ApiError> {
    let todo = timing
        .span(
            "database",
            ctx.todos.find(
                path.organization_id,
                path.todo_id,
                query.include_archived,
            ),
        )
        .await?;

    Ok(Json(todo))
}
```

### Values derived from the request

| Extractor | Source |
|---|---|
| `Path<T>` | Dynamic URL segments |
| `Query<T>` | URL query parameters |
| `Json<T>` | JSON request body |
| `Form<T>` | Form request body |
| `HeaderMap` | Request headers |
| `Method` | HTTP method |
| `Uri` | Complete request URI |
| `Request<Body>` | Low-level request access |

Extractors that consume the request body, such as `Json<T>`, must come last.
The body can only be consumed once:

```rust title="app/api/todos/route.rs"
#[nextrs::api]
pub async fn post(
    Extension(ctx): Extension<AppContext>,
    timing: Timing,
    Json(input): Json<CreateTodo>,
) -> Result<Json<Todo>, ApiError> {
    let todo = timing.span("database", ctx.todos.create(input)).await?;
    Ok(Json(todo))
}
```

### Values provided by the application

Application dependencies are currently installed as Axum extensions when the
shared router is constructed:

```rust title="src/app.rs"
pub fn app() -> axum::Router {
    let context = AppContext {
        db: Database::connect(),
        todos: TodoService::new(),
    };

    nextrs::router::build_router(generated_registry())
        .layer(axum::Extension(context))
}
```

Any route can then request `Extension(ctx): Extension<AppContext>`. The layer
inserts the value into each request before the route runs. This is useful in a
serverless deployment too: configuration, services, and database pools can be
created once per instance and reused by warm invocations.

Axum's typed `State<T>` extractor is not currently supported by the generated
nextrs route registry. Use `Extension<T>` for application dependencies today.

### Values provided by nextrs

Framework middleware makes a few server-only extractors available:

| Extractor | Purpose |
|---|---|
| `Timing` | Add named spans to the `Server-Timing` response header |
| `WaitUntil` | Register work that may continue after the response |
| `Params` | Access nextrs route parameters in lower-level handlers |

The handler does not construct these values. nextrs places the required
context into the request before invoking it.

### What becomes part of the generated client

| Handler value | Generated TypeScript contract |
|---|---|
| `Path<T>` | Typed path arguments |
| `Query<T>` | Typed query object |
| `Json<T>` | Typed request body |
| Success response | Typed response data |
| `ApiError` | Typed error response |
| `Extension<T>`, `Timing`, `WaitUntil` | Server-only; omitted |

Request-contract values become client inputs. Application and framework
context remain server-only.

### Invalid JSON is rejected before the handler runs

`Json<T>` checks the content type, parses the body, and deserializes it before
calling the route function. If extraction fails, the handler is not called and
Axum returns its standard rejection response:

- `415 Unsupported Media Type` for a missing or incorrect JSON content type;
- `400 Bad Request` for malformed JSON syntax;
- `422 Unprocessable Entity` for valid JSON that does not match `T`.

For example, a string sent for a boolean field is valid JSON but the wrong
shape, so Axum responds with `422`. This default is the recommended behavior
for now. A route that needs a custom rejection body can accept
`Result<Json<T>, JsonRejection>` and map the error itself, but that customized
shape must currently declare `request_body = T` explicitly for client
generation.

## Static assets

Files in `public/` are served at the root URL path:

```text
public/logo.svg -> /logo.svg
```

Avoid assigning the same URL to both a public file and a route.
