# react-todos

A standalone nextrs app demonstrating React `page.tsx` pages, typed Rust API
routes, a generated browser client, and a server-seeded React Query cache. The
todo list is available on first paint without a client fetch because
`prefetch.rs` warms the same typed query that the page uses.

## Project layout

```text
app/
├── layout.tsx                  root React layout
├── page.tsx                    todo-list page
├── todo-row.tsx                component colocated with the page that uses it
├── prefetch.rs                 server-side query seed
├── about/page.tsx              unseeded page
├── api/todos/
│   ├── route.rs                typed GET + POST endpoints
│   └── [id]/route.rs           typed GET + PATCH + DELETE endpoints
└── todos/[id]/
    ├── page.tsx                dynamic detail page
    ├── prefetch.rs             typed detail-query seed
    └── test/page.tsx           deep package-resolution and type fixture

components/
└── NextrsLogo.tsx              application-wide shared React component

src/
├── app.rs                      shared Rust application constructor
├── main.rs                     local-process adapter
└── core/todos.rs               framework-independent domain logic

api/index.rs                    Vercel process adapter
build.rs                        route, seed, and browser-bundle generation
.nextrs/
├── dump-openapi.rs             hidden OpenAPI extraction helper
├── openapi.json                generated API contract
└── client/                     generated local npm package
    ├── package.json            stable package exports
    ├── src/                    generated fetch + React Query sources
    └── dist/                   emitted JavaScript and declarations
```

The `app/` directory follows nextrs routing conventions, but it is still
ordinary application source. Components may be colocated beside a page, as
`app/todo-row.tsx` is, or shared through a top-level `components/`
directory. The hidden `.nextrs/client/` directory is generated framework
output; it is not where user-authored React components belong.

## Install, generate, and run

Run all JavaScript commands from this application root
(`examples/react-todos/`). Do not install dependencies inside
`.nextrs/client/`.

```sh
npm ci
npm run client:generate
cargo run -p react-todos
# → http://localhost:3000
```

The root `package.json` declares `.nextrs/client` as a workspace and links
it as the real `@react-todos/client` dependency. A root install creates
`node_modules/@react-todos/client`; this is a genuine local package, not only
a browser-bundler alias.

The entire `.nextrs/client` package and `.nextrs/openapi.json` are ignored
generated state. The tracked `.nextrs/template/client` wiring recreates the
package before generation; `cargo dev` and `nextrs client generate` repair a
missing target before TypeScript or the browser build consumes it.

`npm run client:generate` performs the complete refresh:

1. Run the hidden Rust helper to write `.nextrs/openapi.json`.
2. Generate a framework-agnostic fetch client and a separate React Query
   surface with Orval.
3. Run the nextrs build step, which adds URL-bound hooks and bundles pages.
4. Emit package JavaScript and `.d.ts` declarations into
   `.nextrs/client/dist`.

Run generation again after changing a `#[nextrs::api]` endpoint. Useful
focused checks are:

```sh
npm run client:build
npm run typecheck
cargo build -p react-todos
```

## Use the generated client

Plain fetch functions and wire types come from the framework-agnostic root
entry:

```ts
import {
  getApiTodosById,
  patchApiTodosById,
  type TodoDetail,
} from "@react-todos/client";

const response = await getApiTodosById(42, { neighbors: true });
if (response.status === 200) {
  const todo: TodoDetail = response.data;
  await patchApiTodosById(todo.id, { done: !todo.done });
}
```

React Query hooks, option builders, query keys, URL-bound helpers, and
`useParams` come from the explicit integration entry:

```tsx
import {
  getGetApiTodosByIdQueryOptions,
  useParams,
  usePatchApiTodosById,
} from "@react-todos/client/react-query";

const options = getGetApiTodosByIdQueryOptions(42, { neighbors: true });

function ToggleTodo() {
  const { id } = useParams<{ id: string }>();
  const updateTodo = usePatchApiTodosById();

  return (
    <button
      onClick={() =>
        updateTodo.mutate({ id: Number(id), data: { done: true } })
      }
    >
      Complete
    </button>
  );
}
```

TypeScript resolves both imports through the package's `exports` and
`types` declarations. The app does not use `tsconfig.paths`, relative
imports into generated output, handwritten declaration files, or `any`
annotations.

`app/todos/[id]/test/page.tsx` is deliberately nested several directories
deep. It is an ordinary component that imports and uses both package entry
points without manual client types. The exhaustive compiler checks live where
tests belong: the colocated `page.test.tsx` covers response and error
unions, path and query parameters, request bodies, query data, mutation
variables, invalid inputs, and no-`any` guarantees. The adjacent
`app/client-resolution.test.js` checks the same package entry points from
ordinary JavaScript.

## Rust entry points

`src/app.rs` is the library root and the one place that constructs the
application. Both executable adapters call it:

- `src/main.rs` starts the normal local server.
- `api/index.rs` adapts the same app to Vercel's Rust runtime and adds
  deployment instrumentation. It exists because Vercel currently requires
  that process entry path; applications that do not target Vercel can remove
  the adapter and its Cargo target.

`build.rs` remains normal Rust build-script wiring. It discovers routes,
emits the generated registry and seed companions, and bundles the React pages.
The OpenAPI extraction binary is hidden under `.nextrs/` because it is
framework plumbing rather than application or domain code.

## What to look at

- **No fetch on load** — the todo list is seeded by `prefetch.rs`; the
  component only calls its generated URL-bound hook.
- **End-to-end types** — the `#[nextrs::api]` signatures determine the path,
  query, body, response, error, query-result, and mutation-variable types.
- **One runtime binary** — Rust serves the React pages, static assets, API, and
  `/openapi.json`. Node is required for generation and bundling, not at
  runtime.
- **Thin adapters** — route handlers translate the wire format and delegate to
  `src/core/todos.rs`.
- **Normal component organization** — the example demonstrates both a
  top-level shared component and a component colocated in `app/`.

## Deploy to Vercel

`vercel.json` installs and generates from the application root before its
release Cargo build:

```json
{
  "installCommand": "npm ci",
  "buildCommand": "npm run client:generate && cargo build --release -p react-todos"
}
```

That means Vercel creates the same workspace link, generated declarations, and
browser bundles as local development. The application does not depend on a
manually installed hidden client or a prebuilt committed `public/dist/`
directory.

```sh
vercel deploy --prod
```

`.cargo/config.toml` keeps an empty `[build]` table because the current
`vercel-rust` builder expects that key when a Cargo config exists. Function
runtime selection and the catch-all rewrite remain in `vercel.json`.

See `docs/server-props.md` in the repository root for the server-props and
streaming design.
