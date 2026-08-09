+++
title = "Getting Started"
description = "Set up a React-first nextrs app and run the Cargo-powered dev loop"
section = "Guides"
order = 1
+++

nextrs is a React frontend framework with a Rust backend. Routes live in an
`app/` convention tree, React components use `.tsx`, and Rust `route.rs`
handlers provide typed APIs. A build step discovers and wires everything.

## Create the app

The fastest start is the scaffold:

```bash
cargo install create-nextrs-app
create-nextrs-app mysite
cd mysite
```

The generated app has this shape:

```text
mysite/
├── app/
│   ├── layout.tsx          # shared React layout
│   ├── page.tsx            # /
│   ├── slow/
│   │   ├── page.tsx        # /slow
│   │   ├── loading.tsx     # loading UI
│   │   └── prefetch.rs     # server-warmed React Query data
│   └── api/ping/route.rs   # typed Axum API
├── client/                 # generated fetch functions and React Query hooks
├── public/                 # static assets
├── build.rs                # route discovery and TSX bundling
└── src/main.rs             # Axum server
```

## Your first page

`app/page.tsx` is an ordinary React component:

```tsx
export default function HomePage() {
  return <main>Hello from nextrs</main>;
}
```

Directories become URL segments, so `app/settings/page.tsx` serves
`/settings`. A `layout.tsx` wraps the pages beneath it:

```tsx
import type { ReactNode } from "react";

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <div>
      <nav>My app</nav>
      {children}
    </div>
  );
}
```

The embedded Rolldown-based build bundles these components. There is no
separate frontend build command to remember.

## Add a typed backend route

Create `app/api/greeting/route.rs`:

```rust
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

The annotation opts the handler into client generation. nextrs infers the
method, URL, status, and response body from the function and its location.
Generate both direct fetch functions and React Query hooks with:

```bash
cargo nextrs client generate
```

See [Client Generation: Step by Step](/docs/client-codegen) for progressive
usage examples.

## Server-warm React data

A `prefetch.rs` beside `page.tsx` can fill the page's React Query cache before
the component mounts. It returns a `nextrs::QuerySeed`; the browser receives
those entries with the page shell and hydrates the same keys used by generated
hooks. See [React Pages & Server Prefetch](/docs/react-server-props).

## Run the dev loop

One Cargo installation provides both the watcher and client generator:

```bash
cargo install cargo-nextrs
cargo dev
```

The explicit watcher command is `cargo nextrs dev --bin <crate>`. It rebuilds
and restarts the Rust server when backend or frontend files change. With live
reload enabled, the browser refreshes after the rebuild.

## Where to go next

- [Routing Conventions](/docs/conventions)
- [Client Generation: Step by Step](/docs/client-codegen)
- [Porting an Existing App](/docs/porting)
- [Deploy to Vercel](/docs/deploy-vercel) or [Deploy with Docker](/docs/deploy-docker)
