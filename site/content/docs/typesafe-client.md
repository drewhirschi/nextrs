+++
title = "Generated TypeScript Client"
description = "How typed Rust routes become a linked fetch and React Query package with editor-ready declarations"
section = "Client Generation"
order = 3
+++

nextrs generates client code from the API contract already expressed by your
Rust routes. Rust is the source of truth, OpenAPI is the intermediate format,
and a genuine linked npm package is the application-facing result.

If you want to build one endpoint and call it immediately, start with
[Client Generation: Step by Step](/docs/client-codegen). This page explains
the package and inference contract behind that example.

## The pipeline

```text
app/**/route.rs + #[nextrs::api]
                 |
                 v
          generated_openapi()
                 |
                 v
       .nextrs/openapi.json
                 |
                 v
      .nextrs/client/src/generated/
          | fetch      | react-query
          v            v
       TypeScript emits JavaScript + .d.ts
                 |
                 v
       @my-app/client (linked in root node_modules)
```

Run generation at the application root:

```bash
cargo nextrs client generate
# equivalent: nextrs client generate
```

The command installs application dependencies at the root if `node_modules`
is absent, dumps the current Rust contract, invokes Orval, builds the browser
bundle, and emits the client package's JavaScript and declarations. Do not run
`npm install` inside `.nextrs/client`; it is a generated workspace owned by the
root project.

`cargo dev`, `cargo nextrs dev`, and `nextrs dev` refresh the client before
starting the watcher. Run the explicit generate command when you want a
type-only refresh without starting the app.

## What defines the contract

`#[nextrs::api]` marks a handler as part of the generated-client contract. An
unannotated Axum handler still routes normally, which lets internal callbacks
or health endpoints remain outside the client.

| Contract part | Source in Rust |
|---|---|
| URL | File location, such as `app/api/todos/[id]/route.rs` |
| HTTP method | Handler name such as `get`, `post`, or `patch` |
| Client name | `operation_id`, or a name derived from method and path |
| Path parameters | Axum `Path<T>` extractor and bracketed route segments |
| Query parameters | Axum `Query<T>` extractor |
| Request body | Axum `Json<T>` extractor / documented request body |
| Success body/status | Concrete response type and response declarations |
| Error statuses | `responses(...)` declarations |
| Object schemas | Rust types deriving `utoipa::ToSchema` |

Moving a convention file changes both the route and generated contract. The
URL is not repeated in a separate TypeScript definition.

Document additional statuses because an error's `IntoResponse` implementation
can choose its status at runtime. A handler declaring `200` and `404` produces
a response union that narrows on `response.status`.

## Two stable package entry points

The root export is framework-independent. It contains typed fetch functions,
request/response types, and URL helpers:

```ts
import {
  getApiTodosById,
  updateTodo,
  type GetApiTodosByIdParams,
  type UpdateTodoRequest,
} from "@my-app/client";

const response = await getApiTodosById(42, { neighbors: true });
if (response.status === 200) {
  console.log(response.data.title);
}

await updateTodo(42, { done: true });
```

The `/react-query` export contains hooks, option factories, mutation helpers,
query keys, generated URL-bound hooks, and the same wire types:

```tsx
import {
  getGetApiTodosByIdQueryOptions,
  useGetApiTodosById,
  useUpdateTodo,
} from "@my-app/client/react-query";

const options = getGetApiTodosByIdQueryOptions(42, { neighbors: true });
const todo = useGetApiTodosById(42, { neighbors: true });
const update = useUpdateTodo();

update.mutate({ id: 42, data: { done: true } });
```

All values above are inferred from Rust:

- path and query arguments;
- request bodies and mutation variables;
- success and documented error response unions;
- query `data` and mutation results.

Application code should not annotate generated data or mutation variables as
`any`. Let the generated signatures flow through callbacks and JSX.

## Why imports work in every new file

The scaffold's root `package.json` declares both an npm workspace and a file
dependency on `.nextrs/client`. A root `npm install` therefore links the
generated package into `node_modules/@my-app/client`.

The generated `package.json` publishes explicit exports:

```json
{
  "exports": {
    ".": {
      "types": "./dist/index.d.ts",
      "import": "./dist/index.js"
    },
    "./react-query": {
      "types": "./dist/react-query.d.ts",
      "import": "./dist/react-query.js"
    }
  }
}
```

That is ordinary package resolution, not a nextrs-bundler-only alias. VS Code,
`tsc`, checked JavaScript, and the browser bundler all see the same entry
points. The generated package emits real JavaScript plus `.d.ts` and source
maps; it does not depend on consumers importing raw TypeScript.

A checked JavaScript module gets completion from the same declarations:

```js
// @ts-check
import { getApiTodosById } from "@my-app/client";

const response = await getApiTodosById(42, { neighbors: true });
console.log(response.status === 200 ? response.data.title : "missing");
```

You do not need:

- `tsconfig.paths` entries;
- a handwritten `declare module` shim;
- relative imports into `.nextrs`;
- an `npm install` inside generated output.

## Generated package ownership

Do not edit these by hand. The scaffold's `.gitignore` ignores the complete
generated package, contract, and browser bundle:

```text
.nextrs/client/
.nextrs/openapi.json
public/dist/
```

The tracked `.nextrs/template/client` wiring is framework-owned and recreates
the ignored workspace target. Generation then emits current JavaScript and
declarations before validating both public package exports.

Edit the Rust route or schema and regenerate. Put application React code in
`app/` or `components/`, JavaScript dependencies in the root `package.json`,
and Rust domain logic in `src/`.

## When to regenerate

Regenerate after changing an annotated handler's:

- path or HTTP method;
- path or query parameters;
- request body;
- success or error response;
- referenced schema.

```bash
cargo nextrs client generate
```

## Troubleshooting

If a generated operation is missing:

1. Confirm the handler has `#[nextrs::api]`.
2. Confirm request and response schemas use the relevant serde/utoipa derives.
3. Run generation from the app root.
4. Inspect `.nextrs/openapi.json`: if the operation is absent, fix the Rust
   contract; if present, inspect generator output.

If an import does not resolve:

1. Confirm the root `package.json` depends on `file:./.nextrs/client` and lists
   `.nextrs/client` as a workspace.
2. Run `npm install` once at the root, then generation.
3. Confirm `.nextrs/client/dist/index.d.ts` and `react-query.d.ts` exist.
4. Restart the TypeScript server only after the package is correctly linked;
   do not mask the problem with a `paths` entry.

If `cargo nextrs` is missing:

```bash
cargo install cargo-nextrs
```

## Why OpenAPI

Data-type conversion alone cannot describe URLs, parameter serialization,
request bodies, status-specific errors, or framework integrations. OpenAPI
captures the whole HTTP contract while keeping standard API tooling available.
