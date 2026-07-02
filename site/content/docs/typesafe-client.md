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
                            src/generated/**  (hooks + types)
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

## Generate the client

The client directory holds the orval config and the committed generated output:

```bash
cd site/client
npm install      # first time only
npm run gen      # dump openapi.json from Rust, then run orval
npm run typecheck
```

Both `openapi.json` and `src/generated/**` are committed, so contract changes show up in the diff. Rerun `npm run gen` whenever an annotated `route.rs` changes.

## Use the hooks

Each annotated handler becomes a hook named from its `operation_id` — GETs become query hooks, anything with a body becomes a mutation hook:

```tsx
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useGetApiPing, useSendPing } from "@site/client";

function Ping() {
  const { data } = useGetApiPing();          // GET  /api/ping → typed PingResponse
  const send = useSendPing();                // POST /api/ping → typed PingRequest in

  return (
    <button onClick={() => send.mutate({ data: { message: "hi" } })}>
      {data?.data.message ?? "…"}
    </button>
  );
}

const queryClient = new QueryClient();
export const App = () => (
  <QueryClientProvider client={queryClient}>
    <Ping />
  </QueryClientProvider>
);
```

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

Both flavors come out of the same `npm run gen` pass, and the generated barrel exports them all — new endpoints are importable immediately, with no re-export list to maintain.

## Why OpenAPI

Direct Rust→TS type generation (`ts-rs`, `specta`) only produces *types* — you'd still hand-write the fetch layer and hooks. Going through OpenAPI lets orval generate the entire client (hooks, types, fetchers), keeps the door open to Swagger UI and non-TypeScript consumers, and the file-convention discovery removes utoipa's usual hand-maintained path list.
