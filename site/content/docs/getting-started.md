+++
title = "Getting Started"
description = "Create a React-first nextrs app and learn where application and generated code belong"
section = "Guides"
order = 1
+++

nextrs combines a React frontend with a Rust application. URL structure lives
under `app/`, reusable React UI can live under `components/`, Rust domain logic
lives under `src/`, and framework-generated state stays out of sight under
`.nextrs/`.

## Install one CLI and create the app

```bash
cargo install cargo-nextrs
nextrs new mysite
cd mysite
```

Cargo subcommand users can run the same operation as:

```bash
cargo nextrs new mysite
```

One `cargo install` provides both `nextrs` and `cargo nextrs`. The older
`create-nextrs-app` and `cargo nextrs-dev` launchers remain compatibility
commands, but new projects should use the unified CLI.

The scaffolder installs the root JavaScript dependencies and generates the
typed client before it returns. Its default tree has a deliberate split:

```text
mysite/
├── app/                         # URL tree and route-specific code
│   ├── layout.tsx               # shared React layout
│   ├── page.tsx                 # /
│   ├── PingDemo.tsx             # ordinary colocated component, not a route
│   ├── slow/
│   │   ├── page.tsx             # /slow
│   │   ├── loading.tsx          # pending UI
│   │   └── prefetch.rs          # server-warmed React Query data
│   └── api/ping/route.rs        # typed Axum API
├── components/                  # React UI shared by multiple routes
├── src/
│   ├── app.rs                   # shared Rust Router and application wiring
│   └── main.rs                  # local/container process entry
├── .nextrs/                     # generated framework state; do not edit
│   ├── client/                  # linked generated npm package
│   └── dump-openapi.rs          # hidden code-generation helper
├── api/index.rs                 # Vercel process adapter
├── public/                      # static assets
├── build.rs                     # route discovery and browser bundling
├── package.json                 # all JavaScript dependencies live here
└── vercel.json                  # Vercel build and routing configuration
```

The mental model is:

- `app/` describes URLs. Only recognized convention filenames create routes.
  Put a component beside the page that alone uses it.
- `components/` holds React components shared across routes. It is a useful
  default, not a restriction.
- `src/` is the Rust application and domain layer. Add ordinary Rust modules
  here.
- `.nextrs/` is generated. Import its package; do not write application code
  there or run `npm install` inside it.

## Your first page

`app/page.tsx` is an ordinary React component:

```tsx
import { NextrsLogo } from "@/components/NextrsLogo";

export default function HomePage() {
  return <main><NextrsLogo /> Hello from nextrs</main>;
}
```

Directories become URL segments, so `app/settings/page.tsx` serves
`/settings`. You can freely colocate supporting files:

```text
app/settings/page.tsx
app/settings/SettingsForm.tsx
app/settings/format-preferences.ts
```

Only `page.tsx` is a convention file; the other two are normal modules. Move a
component to top-level `components/` when several routes share it.

## Add a typed Rust endpoint

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

`#[nextrs::api]` opts the handler into the generated OpenAPI contract. nextrs
derives its method and URL from `get` and `app/api/greeting/route.rs`, then
Orval generates two stable entry points.

Use the package root for framework-independent fetch functions:

```ts
import { getApiGreeting } from "@mysite/client";

const response = await getApiGreeting();
console.log(response.data.message);
```

Use `/react-query` for hooks, query options, query keys, and mutations:

```tsx
import { useGetApiGreeting } from "@mysite/client/react-query";

export function Greeting() {
  const greeting = useGetApiGreeting();
  return <p>{greeting.data?.data.message}</p>;
}
```

After changing an annotated endpoint, regenerate from the project root:

```bash
cargo nextrs client generate
# equivalent: nextrs client generate
```

`.nextrs/client` is a genuine npm workspace dependency linked into root
`node_modules`. Its package exports point to built JavaScript and `.d.ts`
declarations, so a brand-new nested `.ts` or `.tsx` file resolves both imports
in TypeScript and VS Code. No relative generated import, declaration shim, or
`tsconfig.paths` entry is required.

`nextrs new` also creates the root `.gitignore`. The whole generated
`.nextrs/client` package is ignored. A small tracked template under
`.nextrs/template/client` lets `cargo dev` and client generation recreate the
package before TypeScript or the browser build consumes it.

## Run the dev loop

The default shortcut is:

```bash
cargo dev
```

These direct forms run the same watcher:

```bash
cargo nextrs dev
nextrs dev
```

`cargo dev` is a scaffolded Cargo alias. The unified dev command refreshes the
generated client, builds the app, starts it, and watches relevant Rust,
frontend, template, asset, and environment files.

## Why `app.rs`, `main.rs`, `api/index.rs`, and `build.rs` all exist

- `src/app.rs` constructs the shared Axum `Router`. Application-wide layers
  and domain wiring belong here.
- `src/main.rs` only starts the local/container process and calls that shared
  app.
- `api/index.rs` is a thin Vercel adapter required by Vercel's current Rust
  entry convention. Do not put application logic there.
- `build.rs` is normal Rust build-script infrastructure. It discovers the
  `app/` tree, generates the route/OpenAPI registry, and bundles React pages.

If Vercel is not a deployment target, remove `api/index.rs`, its `index` Cargo
target, the Vercel-only dependencies, `vercel.json`, and the prebuilt-deploy
script together. Keep `src/app.rs`, `src/main.rs`, and `build.rs`.

## Where to go next

- [Routing Conventions](/docs/conventions)
- [A Rust-First Tour](/docs/rust-first-tour)
- [Client Generation: Step by Step](/docs/client-codegen)
- [Porting an Existing App](/docs/porting)
- [Deploy to Vercel](/docs/deploy-vercel) or [Deploy with Docker](/docs/deploy-docker)
