+++
title = "Client Generation: Step by Step"
description = "Turn typed Rust path, query, body, response, and error contracts into fetch functions and React Query hooks"
section = "Client Generation"
order = 4
+++

This walkthrough starts with one Rust endpoint, then adds the inputs and error
cases that demonstrate end-to-end inference. The generated client requires no
`any`, handwritten interface, relative generated import, or module shim.

## 1. Write a typed Rust endpoint

Create `app/api/todos/[id]/route.rs`:

```rust
use axum::{extract::{Path, Query}, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Deserialize, IntoParams)]
pub struct TodoQuery {
    pub neighbors: Option<bool>,
}

#[derive(Serialize, ToSchema)]
pub struct Todo {
    pub id: u64,
    pub title: String,
    pub done: bool,
}

#[nextrs::api(
    get,
    responses(
        (status = 200, description = "The todo", body = Todo),
        (status = 404, description = "Not found"),
    ),
)]
pub async fn get(
    Path(id): Path<u64>,
    Query(query): Query<TodoQuery>,
) -> Result<Json<Todo>, StatusCode> {
    find_todo(id, query.neighbors.unwrap_or(false))
        .await
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}
```

The `[id]` directory and `Path<u64>` define the path argument. `Query<TodoQuery>`
defines query options. The response declarations produce a status union. The
default operation ID is derived from method and path; set `operation_id` in the
annotation when you want a shorter public name.

This remains an ordinary Axum handler. The attribute adds it to the OpenAPI
document; it does not create a second RPC runtime.

## 2. Generate from the app root

Install the unified CLI once:

```bash
cargo install cargo-nextrs
cargo nextrs client generate
```

`nextrs client generate` is equivalent. Generation refreshes the Rust
contract, produces both fetch and React Query surfaces, runs the application
build, and emits JavaScript and `.d.ts` files for the linked client package.

Never run `npm install` in `.nextrs/client`. The root project owns dependencies
and links this generated workspace.

## 3. Call the framework-independent client

```ts
import { getApiTodosById } from "@mysite/client";

const response = await getApiTodosById(42, { neighbors: true });

if (response.status === 200) {
  console.log(response.data.title);
} else {
  console.log("Todo was not found");
}
```

The path argument must be a number; `neighbors` must be a boolean when
present; and the `200` branch carries `Todo`. TypeScript rejects invalid calls
at the call site.

The package root uses the platform `fetch` API and has no React dependency in
its public surface. Use it in browser modules, event handlers, or another UI
framework.

## 4. Use React Query integration

React-specific APIs live at the explicit subpath:

```tsx
import {
  getGetApiTodosByIdQueryOptions,
  useGetApiTodosById,
} from "@mysite/client/react-query";

export function TodoDetail({ id }: { id: number }) {
  const todo = useGetApiTodosById(id, { neighbors: true });

  if (todo.isPending) return <p>Loading…</p>;
  if (todo.data?.status !== 200) return <p>Not found</p>;
  return <p>{todo.data.data.title}</p>;
}

const options = getGetApiTodosByIdQueryOptions(42, { neighbors: true });
```

Query data is inferred from the fetch function. There is no need to write a
response generic or annotate callback data.

## 5. Add a typed request body and mutation

Add a patch handler to the same `route.rs`:

```rust
#[derive(Deserialize, ToSchema)]
pub struct UpdateTodoRequest {
    pub done: bool,
}

#[nextrs::api(
    patch,
    operation_id = "updateTodo",
    request_body = UpdateTodoRequest,
    responses((status = 200, description = "Updated todo", body = Todo)),
)]
pub async fn patch(
    Path(id): Path<u64>,
    Json(body): Json<UpdateTodoRequest>,
) -> Json<Todo> {
    Json(update_todo(id, body.done).await)
}
```

Regenerate and use the direct client:

```ts
import { updateTodo } from "@mysite/client";

await updateTodo(42, { done: true });
```

Or let the generated mutation infer its variables:

```tsx
import { useUpdateTodo } from "@mysite/client/react-query";

const update = useUpdateTodo({
  mutation: {
    onSuccess: (_response, variables) => {
      console.log(variables.id, variables.data.done);
    },
  },
});

update.mutate({ id: 42, data: { done: true } });
```

`variables.id` and `variables.data` are inferred from `Path<u64>` and
`UpdateTodoRequest`. Do not annotate either as `any`.

## 6. Watch a Rust change reach TypeScript

Rename `Todo.title` to `Todo.label`, then regenerate:

```bash
cargo nextrs client generate
```

Every stale `.title` use now fails at the exact consumer. That is the intended
feedback loop: one Rust-owned contract drives fetch calls, query results,
mutation variables, status unions, and editor completion.

## 7. Use imports from any nested file

The generated package is a linked root dependency with explicit exports. A
new file such as `app/todos/[id]/details/page.tsx` uses the same stable imports:

```tsx
import { getApiTodosById } from "@mysite/client";
import { useUpdateTodo } from "@mysite/client/react-query";
```

TypeScript reads `.nextrs/client/dist/index.d.ts` and
`dist/react-query.d.ts`; JavaScript and the browser bundler read the matching
`.js` files. Resolution does not depend on a page already existing, a nextrs
runtime alias, or user-authored `tsconfig.paths`.

## The rule to remember

After changing a `#[nextrs::api]` contract, regenerate from the application
root:

```bash
cargo nextrs client generate
```

For package ownership, troubleshooting, and contract mapping, read the
[Generated TypeScript Client](/docs/typesafe-client) reference.
