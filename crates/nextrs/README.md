# nextrs

A React-first, Next.js-style application framework for Rust, built on
[Axum](https://docs.rs/axum) and [Askama](https://docs.rs/askama).

nextrs provides file-based routes, React pages and layouts, Rust/HTML pages,
streamed loading UI, server-seeded React Query data, and a generated
OpenAPI-based TypeScript client. It runs as a normal Axum app or as one Vercel
Rust function.

## Quick start

Install one CLI, then use either launcher:

```bash
cargo install cargo-nextrs
nextrs new my-app
# equivalent: cargo nextrs new my-app
cd my-app
cargo dev
```

`cargo dev`, `cargo nextrs dev`, and `nextrs dev` run the same development
workflow. The scaffold installs root JavaScript dependencies and generates the
typed client automatically.

## Application layout

```text
my-app/
├── app/                         URL structure and route-local code
│   ├── layout.tsx               shared React layout
│   ├── page.tsx                 /
│   ├── TodoRow.tsx              ordinary colocated component, not a route
│   └── api/todos/[id]/route.rs  typed Axum endpoint
├── components/                  shared React components
├── src/
│   ├── app.rs                   shared Router and Rust application wiring
│   └── main.rs                  local/container process entry
├── .nextrs/client/              generated linked npm package; do not edit
├── api/index.rs                 thin Vercel adapter
├── public/                      static assets
└── build.rs                     route/OpenAPI discovery and TSX bundling
```

Directories below `app/` become URL segments. Only exact convention filenames
have framework meaning:

- `page.{tsx,rs,html}`
- `layout.{tsx,rs,html}`
- `loading.{tsx,rs,html}`
- `not-found.{tsx,rs,html}`
- `middleware.rs`
- `route.rs`
- `prefetch.rs` beside a `page.tsx`

Other files may be freely colocated. A `.tsx` slot cannot coexist with the
Rust/HTML form of the same slot.

## Generated API client

Annotate an ordinary typed handler:

```rust,ignore
#[nextrs::api]
pub async fn get(Path(id): Path<u64>) -> Json<Todo> {
    Json(load_todo(id).await)
}
```

Then regenerate from the application root:

```bash
cargo nextrs client generate
```

The scaffold links `.nextrs/client` into root `node_modules` and publishes two
stable entry points:

```ts
import { getApiTodosById } from "@my-app/client";
import {
  getGetApiTodosByIdQueryOptions,
  useGetApiTodosById,
} from "@my-app/client/react-query";
```

Path/query inputs, request bodies, response and error unions, query data, and
mutation variables are inferred from Rust. The package emits JavaScript and
`.d.ts`; it needs no relative generated imports, declaration shims, or
`tsconfig.paths`. Install JavaScript dependencies only at the app root, never
inside `.nextrs/`.

## Rust application entry points

`src/app.rs` owns the shared Axum `Router`. `src/main.rs` starts it locally.
The generated `api/index.rs` adapts the same app to Vercel's required entry and
should contain no application logic. `build.rs` is normal Rust build-script
infrastructure for route/OpenAPI generation and browser bundling.

If Vercel is not a deployment target, remove `api/index.rs`, its Cargo target,
Vercel-only dependencies, `vercel.json`, and the prebuilt deploy script
together. Keep the shared app, local entry, and build script.

## Cargo features

- **`build`** — route discovery, registry/OpenAPI generation, and the docs
  pipeline. Use it under `[build-dependencies]`.
- **`tsx`** — build-time React page bundling through embedded Rolldown. Enable
  it with `build` for apps using `.tsx` convention files.
- **`vercel`** — `StreamingVercelLayer`, which adapts Axum to the Vercel
  runtime while preserving streamed HTML.

## Streaming and prefetch

A `loading` slot can reach the browser before a slow page finishes. For React
pages, a sibling `prefetch.rs` runs the typed endpoint on the server and seeds
the exact React Query key the generated hook uses. Hard loads and soft
navigation therefore share one endpoint contract and one cache shape.

## Status

Pre-1.0—the API can change. See the
[documentation site](https://nextrs-docs.vercel.app/docs/getting-started) and
the repository examples for the complete guides.

## License

Apache-2.0
