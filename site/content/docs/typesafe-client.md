+++
title = "Typesafe Client Generation"
description = "Generate a typed TypeScript / React Query client from your route.rs handlers"
section = "Guides"
order = 4
+++

nextrs can generate a fully-typed TypeScript client — TanStack (React) Query hooks with typed request and response shapes — directly from your `route.rs` handlers. Rename a field in Rust and the TypeScript call sites stop compiling. The pipeline is OpenAPI-based:

```
route.rs (#[nextrs::api])  ─codegen→  generated_openapi()
        │                                     │
        │                       cargo run --bin dump-openapi
        ▼                                     ▼
   served at /openapi.json            client/openapi.json
                                              │
                                            orval
                                              ▼
                     src/generated/basic/**  (fetch client + types)
                src/generated/react-query/** (hooks + types)
                                              │
                                            tsc
                                              ▼
                                 client/dist/** (JS + .d.ts)
```

## Annotate a handler

Handlers stay ordinary Axum handlers — typed extractors in, concrete return types out. Add `#[nextrs::api]` to the ones you want in the client:

```rust
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema)]
pub struct PingResponse {
    pub message: String,
    pub pong: bool,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct PingRequest {
    pub message: String,
}

#[nextrs::api(
    get,
    responses((status = 200, description = "Pong", body = PingResponse)),
)]
pub async fn get() -> Json<PingResponse> {
    Json(PingResponse { message: "pong".into(), pong: true })
}

#[nextrs::api(
    post,
    operation_id = "sendPing",
    responses((status = 200, description = "Echoes the message", body = PingResponse)),
)]
pub async fn post(Json(req): Json<PingRequest>) -> Json<PingResponse> {
    Json(PingResponse { message: req.message, pong: true })
}
```

`#[nextrs::api]` is a thin wrapper over `#[utoipa::path]` that derives the URL from the file's location (`app/api/ping/route.rs` → `/api/ping`), so the path is never restated and can't drift from the file convention. You write the method, `responses(...)` (response types aren't inferred from the return type), and optionally `operation_id` / `tag` for nicer hook names. The request body **is** inferred from the `Json<T>` extractor.

Annotation is **opt-in per handler**: an un-annotated handler still routes and serves normally — it just doesn't appear in the spec or the generated client.

## The spec

The same build-time discovery that wires your routes collects the annotated handlers into a `generated_openapi()` function. The app serves the document at `/openapi.json`, and a `dump-openapi` binary writes the identical spec to `client/openapi.json` so the client can be generated offline.

## Install and generate

Run these commands at the application root, not inside `client/`:

```bash
npm install                 # first time: links the client workspace
npm run client:generate     # after adding or changing a #[nextrs::api] handler
npm run typecheck
```

`client:generate` dumps the Rust OpenAPI document, generates both clients,
refreshes their barrels, and emits JavaScript plus declaration files. You do
not install dependencies separately in `client/`, write a declaration file,
or add `tsconfig.paths`. The root workspace links the package into
`node_modules`, so TypeScript and VS Code resolve it from every `.ts` or `.tsx`
file, including newly created nested pages.

Rerun `npm run client:generate` whenever an annotated handler's parameters,
body, response, error response, or `operation_id` changes.

## Use the hooks

Each annotated handler becomes a hook named from its `operation_id` — GETs become query hooks, anything with a body becomes a mutation hook:

```tsx
import { useGetApiPing, useSendPing } from "@site/client/react-query";

function Ping() {
  const { data } = useGetApiPing();          // GET  /api/ping → typed PingResponse
  const send = useSendPing();                // POST /api/ping → typed PingRequest in

  return (
    <button onClick={() => send.mutate({ data: { message: "hi" } })}>
      {data?.data.message ?? "…"}
    </button>
  );
}

```

nextrs mounts pages under its `QueryClientProvider`; application pages should
not add a second provider. Query data, errors, and mutation variables are
inferred. Avoid annotations such as `data: any` or `variables: any`—they erase
the generated contract.

The generated client uses the platform `fetch` (no HTTP-library dependency) and same-origin URLs — the nextrs app serves both the pages and the API, so there's no CORS story to manage.

## Or skip the hooks: plain typed clients

Every endpoint also gets a framework-free typed function alongside its hook — same types, no React Query, no component context required. Reach for these in event handlers, scripts, and tests instead of raw `fetch` (which re-duplicates route strings, request shapes, and response parsing by hand):

```ts
import { getSources, updateSource } from "@site/client";

// In an event handler — no hook, still fully typed end to end.
async function archive(id: number) {
  const source = await getSources();                       // GET, typed response
  await updateSource(id, { status: "archived" });          // PATCH, typed body
}
```

The root package export is deliberately framework-agnostic. React Query hooks,
query keys, and query-option factories live under `/react-query`:

```ts
import { getApiTodosById } from "@site/client";
import {
  getGetApiTodosByIdQueryOptions,
  useUpdateTodo,
} from "@site/client/react-query";

const detail = await getApiTodosById(42, { neighbors: true });
const options = getGetApiTodosByIdQueryOptions(42, { neighbors: true });

const update = useUpdateTodo();
update.mutate({ id: 42, data: { done: true } });
```

Path parameters, query objects, bodies, successful and error responses, query
results, and mutation variables all flow from the Rust endpoint. Names derive
from OpenAPI `operation_id`; set it explicitly when you want a concise stable
name such as `getTodo`.

Both flavors come out of the same `npm run client:generate` pass. New endpoints
are importable immediately, with no hand-maintained re-export list.

## If an import does not resolve

From the application root, check these in order:

1. Run `npm install` and confirm `node_modules/@scope/client` links to `client/`.
2. Run `npm run client:generate` and confirm `client/dist/index.d.ts` and
   `client/dist/react-query.d.ts` exist.
3. Restart the TypeScript server only if the files resolve on disk but an
   already-open editor still shows a stale diagnostic.

Installing inside `client/` does not link the package into the application and
is not part of the supported workflow.

## Why OpenAPI

Direct Rust→TS type generation (`ts-rs`, `specta`) only produces *types* — you'd still hand-write the fetch layer and hooks. Going through OpenAPI lets orval generate the entire client (hooks, types, fetchers), keeps the door open to Swagger UI and non-TypeScript consumers, and the file-convention discovery removes utoipa's usual hand-maintained path list.
