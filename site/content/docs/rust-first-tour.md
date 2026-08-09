+++
title = "A Rust-First Tour"
description = "Build from one line of HTML to local state, a typed Rust API call, React Query, and server-seeded data"
section = "Guides"
order = 2
+++

nextrs is easiest to understand one layer at a time. You do not need React,
React Query, client generation, or even a Rust page handler to put something on
the screen. Add each only when the problem in front of you needs it.

This tour deliberately builds the same idea five times. It is also a useful
demo path: start with almost nothing, put Rust behind an HTTP boundary, and
then show how that Rust contract keeps the richer client honest.

## 1. A page with text

Create `app/page.html`:

```html
<h1>Hello from nextrs</h1>
```

That is a complete page. The file convention creates `/`; no component,
JavaScript, or Rust handler is required.

## 2. A page with local state

Rename it to `app/page.tsx` when the page needs browser interaction:

```tsx
import { useState } from "react";

export default function Counter() {
  const [count, setCount] = useState(0);

  return (
    <button onClick={() => setCount((value) => value + 1)}>
      Clicked {count} times
    </button>
  );
}
```

This state is entirely local. Rust serves the page and its bundle, but there is
no backend data yet.

## 3. Put the contract in Rust

Add `app/api/todos/route.rs`:

```rust
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema)]
pub struct Todo {
    pub id: u64,
    pub title: String,
    pub done: bool,
}

#[nextrs::api(
    get,
    operation_id = "getTodos",
    responses((status = 200, description = "List todos", body = Vec<Todo>)),
)]
pub async fn get() -> Json<Vec<Todo>> {
    Json(vec![Todo {
        id: 1,
        title: "Learn nextrs".into(),
        done: false,
    }])
}
```

The handler is an ordinary typed Axum handler. `#[nextrs::api]` adds this Rust
contract to the generated OpenAPI document; the URL still comes from the file
location, so it is not repeated in the annotation.

After changing the contract, generate the client from the application root:

```bash
cargo nextrs client generate
```

## 4. Call Rust directly from TypeScript

The generated client does not require React Query. Import its plain function
from the package root and call it like any other async function:

```tsx
import { getTodos } from "@mysite/client";
import { useState } from "react";

export default function Todos() {
  const [message, setMessage] = useState("Nothing loaded yet");

  async function load() {
    const response = await getTodos();
    setMessage(response.data[0]?.title ?? "No todos");
  }

  return <button onClick={load}>{message}</button>;
}
```

There is no handwritten URL, response interface, cast, or generic annotation.
The generated `getTodos` function and its result come from the Rust endpoint.
Rename `title` in Rust, regenerate, and this page stops type-checking at the
exact place that still expects the old contract.

Use this plain client in event handlers, scripts, tests, or any UI framework.
It is the smallest end-to-end example of TypeScript consuming a Rust-owned API.

## 5. Add React Query when server state grows

React Query becomes useful when the page needs caching, loading states,
refetching, mutations, or invalidation. The hook is generated beside the plain
function from the same contract:

```tsx
import { useGetTodos } from "@mysite/client";

export default function Todos() {
  const { data, isPending } = useGetTodos();

  if (isPending) return <p>Loading…</p>;

  return (
    <ul>
      {data?.data.map((todo) => <li key={todo.id}>{todo.title}</li>)}
    </ul>
  );
}
```

The Rust handler has not changed. The direct function and the hook are two
ways to consume the same generated API, not two competing backend designs.

## 6. Seed the first render from Rust

Finally, add a sibling `prefetch.rs` when the first render should already have
the query result. nextrs runs that work on the server and seeds the same
canonical query key used by `useGetTodos`. The component above does not need a
special server-data prop or a second data path.

This is the progression used by the
[`react-todos`](https://github.com/drewhirschi/nextrs/tree/main/examples/react-todos)
example:

```
HTML page
  → React local state
  → typed Rust route
  → plain generated client
  → generated React Query hook
  → Rust-seeded first render
```

Every step earns the next abstraction. Rust remains the source of truth for
the backend contract, while the frontend can stay tiny or grow into a full
interactive application without duplicating that contract.

Next, read [Client Code Generation, Step by Step](/docs/client-codegen) for the
smallest complete client workflow, [Typesafe Client Generation](/docs/typesafe-client)
for the full reference, and [Server Data in React](/docs/react-server-props)
for prefetching, streaming, and cache-key details.
