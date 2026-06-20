# `@site/client` — generated TypeScript / React Query client

A typesafe client for the nextrs app's API, generated from its OpenAPI document.
Nothing here is written by hand except the config.

## Pipeline

```
route.rs (#[utoipa::path])  ─codegen→  generated_openapi()
        │                                     │
        │                     cargo run --bin dump-openapi
        ▼                                     ▼
   served at /openapi.json            client/openapi.json
                                              │
                                            orval
                                              ▼
                          src/generated/**  (hooks + types)
```

## Regenerate

```sh
cd site/client
npm install      # first time only
npm run gen      # dump openapi.json from Rust, then run orval
npm run typecheck
```

`npm run gen` runs two steps:

1. `dump` — `cargo run -p site --bin dump-openapi`, which serializes
   `generated_openapi()` to [`openapi.json`](./openapi.json).
2. `orval` — turns that spec into hooks under `src/generated/`
   (config: [`orval.config.ts`](./orval.config.ts)).

Both `openapi.json` and `src/generated/**` are committed so the client is
reviewable in the diff; rerun `npm run gen` whenever a `route.rs` contract
changes.

## Usage

Each annotated handler becomes a hook named from its `operation_id`. A `GET`
becomes a query hook, anything with a body becomes a mutation hook:

```tsx
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useGetApiPing, useSendPing } from "@site/client";

function Ping() {
  // GET /api/ping  → fully typed PingResponse
  // (operation_id derived from the route → useGetApiPing)
  const { data } = useGetApiPing();

  // POST /api/ping → typed PingRequest in, PingResponse out
  // (operation_id overridden to "sendPing" → useSendPing)
  const send = useSendPing();

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

## Opt-in per handler

A `route.rs` handler is only added to the client if it carries `#[nextrs::api]`
(or a raw `#[utoipa::path]`). Leave the attribute off and the handler still
routes and serves normally — it just won't appear in `openapi.json` or the
generated hooks. Nothing breaks; the endpoint is simply untyped from the
client's perspective.

Each build writes an inspection summary under `target/nextrs/`, so an
unannotated endpoint is visible without turning routine Cargo output into
warnings. Set `NEXTRS_VERBOSE=1` to also echo the same summary during codegen.

```
nextrs: typed client generated for 2/3 route.rs handler(s)
  GET     /api/ping                client ✓
  POST    /api/ping                client ✓
  GET     /api/health              no client (add #[nextrs::api])
```

The generator emits same-origin `fetch` calls (`baseUrl: "/"`), so it works
against the app serving it with no extra config. Adding or changing a
`#[utoipa::path]` handler and rerunning `npm run gen` updates the hooks and
types in lockstep — a request/response field rename surfaces as a TypeScript
error at the call site.
