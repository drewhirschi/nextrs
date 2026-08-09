+++
title = "Routing Conventions"
description = "React pages, layouts, loading UI, middleware, APIs, and server prefetch"
section = "Guides"
order = 2
+++

Every directory under `app/` is a URL segment. These are the public React-first
conventions:

| File | Role |
|---|---|
| `page.tsx` | React content for the URL |
| `layout.tsx` | Shared React UI around this segment and its descendants |
| `loading.tsx` | Immediate loading UI while route data becomes available |
| `prefetch.rs` | Server data that warms the page's React Query cache |
| `middleware.rs` | Request guard or transformation |
| `route.rs` | Typed Axum API handlers |

## Pages

```tsx
export default function UsersPage() {
  return <h1>Users</h1>;
}
```

`app/users/page.tsx` maps to `/users`. nextrs bundles each page and mounts it
under a TanStack `QueryClientProvider`.

## Layouts

Layouts nest from root to leaf and receive the matched page as `children`:

```tsx
import type { ReactNode } from "react";

export default function DashboardLayout({ children }: { children: ReactNode }) {
  return (
    <section>
      <aside>Dashboard</aside>
      <main>{children}</main>
    </section>
  );
}
```

`app/layout.tsx` wraps the whole app. `app/dashboard/layout.tsx` adds another
layer only for routes below `/dashboard`.

## Loading UI

Place `loading.tsx` beside a page to define its pending experience:

```tsx
export default function Loading() {
  return <p>Loading…</p>;
}
```

Keep loading components free of data dependencies. The resolved page replaces
them when its prefetched data and route code are ready.

## Server prefetch

A `prefetch.rs` beside `page.tsx` returns a `nextrs::QuerySeed`. On a hard load,
the server sends those cache entries with the React shell. On hover and soft
navigation, the app shell warms both the route chunk and its data automatically.
See [React Pages & Server Prefetch](/docs/react-server-props) for the complete
flow.

## Middleware

`middleware.rs` files compose root-to-leaf and run before pages and API
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

## API routes

`route.rs` exports an async function named for each HTTP method. Axum
extractors define the inputs and concrete response types define the output:

```rust
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct Pong { pub message: String }

#[nextrs::api]
pub async fn get() -> Json<Pong> {
    Json(Pong { message: "pong".into() })
}
```

See [Client Generation: Step by Step](/docs/client-codegen) for consuming the
generated function directly or through React Query.

## Dynamic segments

A bracketed directory matches one path segment:

```text
app/users/[id]/page.tsx       → /users/{id}
app/api/users/[id]/route.rs   → /api/users/{id}
```

The React route receives the matched route context, while an API handler reads
the same value through Axum's `Path<T>` extractor.

## Static assets

Files in `public/` are served at the root URL path:

```text
public/logo.svg → /logo.svg
```

Avoid assigning the same URL to both a public file and a route.
