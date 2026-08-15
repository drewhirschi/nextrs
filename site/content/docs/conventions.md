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
| `page.tsx` | Client-rendered React page for this URL |
| `page.rs` + optional `page.html` | Server-rendered Rust/Askama page |
| `page.html` | Static HTML page when no `page.rs` exists |
| `layout.tsx` | React layout for this segment and descendants |
| `layout.rs` + `layout.html` | Server-rendered layout |
| `loading.tsx` | React pending UI |
| `loading.rs` + optional `loading.html` | Server-rendered loading shell |
| `not-found.tsx` | React 404 surface for this subtree |
| `not-found.rs` + optional `not-found.html` | Server-rendered 404 surface |
| `middleware.rs` | Request guard or transformation |
| `route.rs` | Axum handlers named for HTTP methods |
| `prefetch.rs` | Server data that warms a sibling `page.tsx` query cache |

A `.tsx` rendering slot cannot coexist with the Rust/HTML form of the same
slot. For example, choose either `page.tsx` or `page.rs`/`page.html` in one
segment. `prefetch.rs` requires a `page.tsx` sibling because it feeds that
React page's cache.

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

## Static assets

Files in `public/` are served at the root URL path:

```text
public/logo.svg -> /logo.svg
```

Avoid assigning the same URL to both a public file and a route.
