# nextrs

**nextrs** is a Rust web framework for building React apps. It combines
file-based frontend routing, generated TypeScript clients, TanStack React Query,
and a Rust/Axum backend in one Cargo-driven project.

> The React frontend is the supported path. Earlier non-React frontend
> conventions are deprecated and are no longer documented for new apps.

## Project anatomy

```text
my-app/
├── app/
│   ├── layout.tsx             shared React layout
│   ├── page.tsx               /
│   ├── todos/
│   │   ├── page.tsx           /todos
│   │   ├── TodoRow.tsx        colocated, non-route component
│   │   ├── loading.tsx        loading UI
│   │   └── prefetch.rs        server-warmed React Query data
│   └── api/todos/route.rs     typed Axum API
├── components/                shared React components
├── src/
│   ├── app.rs                 shared Rust application/router
│   └── main.rs                local and container process entry
├── .nextrs/
│   ├── client/                generated TypeScript package; do not edit
│   └── dump-openapi.rs        framework codegen helper
├── api/index.rs               Vercel process adapter
├── public/                    static assets
└── build.rs                   Cargo build hook for routes and TSX
```

Each directory is a route segment. The supported frontend conventions are:

| File | Purpose |
|---|---|
| `page.tsx` | React content for a route |
| `layout.tsx` | Shared React UI around nested routes |
| `loading.tsx` | Pending UI while route code and data become available |
| `prefetch.rs` | Server-side warming for the page's React Query cache |
| `middleware.rs` | Rust request guards and transformations |
| `route.rs` | Rust/Axum API handlers |

Only convention filenames participate in routing. Other modules can be
colocated freely in `app/`; put React components shared across routes in
`components/`. Rust domain and application logic belongs in `src/`. Everything
under `.nextrs/` is framework wiring and should not be edited. The generated
`.nextrs/client` package is ignored; `nextrs new`, client generation, and
`cargo dev` materialize it from the small tracked framework template.

The embedded Rolldown-based builder bundles the React frontend. The Rust
binary serves the application and APIs; no Node server runs in production.

## A React page

```tsx
// app/page.tsx
export default function HomePage() {
  return <h1>Hello from nextrs</h1>;
}
```

## A typed Rust API

```rust
// app/api/greeting/route.rs
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct Greeting {
    pub message: String,
}

#[nextrs::api]
pub async fn get() -> Json<Greeting> {
    Json(Greeting { message: "Hello from Rust".into() })
}
```

`#[nextrs::api]` opts the handler into client generation. nextrs infers the
HTTP method, path, success status, and response body. Generate a direct fetch
function and React Query hook with:

```bash
cargo nextrs client generate
```

## Start an app

```bash
cargo install cargo-nextrs
nextrs new my-app
cd my-app
cargo dev
```

`nextrs new` installs the root npm dependencies and generates the typed client.
Pass `--no-install` to write files only and print the two bootstrap commands.

One `cargo-nextrs` installation provides creation, the dev watcher, and client
generation. Every command supports both launch forms:

```bash
nextrs new my-app
# equivalent: cargo nextrs new my-app

nextrs dev
# equivalent: cargo nextrs dev

nextrs client generate
# equivalent: cargo nextrs client generate
```

Scaffolded projects retain `cargo dev` as a short Cargo alias. The old
`create-nextrs-app` and `cargo nextrs-dev` commands remain compatibility
wrappers but are deprecated.

## Server-prefetched React data

A `prefetch.rs` beside `page.tsx` can call the typed Rust handler directly and
seed its result under the same query key used by the generated React Query
hook. The component stays ordinary React Query code; without `prefetch.rs`, it
simply fetches on mount.

See the runnable [`react-todos`](examples/react-todos) app for the complete
pattern.

## Repository

```text
crates/nextrs/             framework and build pipeline
crates/nextrs-macros/      #[nextrs::api]
crates/cargo-nextrs/       unified `nextrs` / `cargo nextrs` CLI
crates/create-nextrs-app/  scaffold library + deprecated wrapper
crates/cargo-nextrs-dev/   dev runner library + deprecated wrapper
examples/react-todos/      end-to-end example
site/                      documentation site and demo app
```

Run the workspace tests with:

```bash
cargo test --workspace --all-features
```

The full guides live in the [documentation site](https://nextrs.dev/docs/getting-started).
Future work lives in [ROADMAP.md](ROADMAP.md).
