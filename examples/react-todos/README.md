# react-todos

A standalone nextrs app demonstrating **React `page.tsx` pages** with a
**server-seeded React Query cache** — the list renders on first paint with no
client fetch, because `props.rs` warmed the cache from the same stream that
delivered the HTML.

```
app/
├── layout.{rs,html}        root layout + hand-written stylesheet
├── page.tsx                the React page (client-rendered)
├── props.rs                seeds the open-todos query into the cache
└── api/todos/
    ├── route.rs            GET (list) + POST (add)  — #[nextrs::api]
    └── [id]/route.rs       DELETE (remove)          — #[nextrs::api]
src/core/todos.rs           in-memory domain layer (the handlers are thin adapters over it)
client/                     orval-generated typed React Query hooks
```

## Run it

```sh
# 1. Install the client deps + generate the typed hooks (first time / after API changes)
cd client && npm install && npm run gen && cd ..

# 2. Run the app
cargo run -p react-todos
# → http://localhost:3000
```

`cargo build` bundles `page.tsx` (via rolldown, from inside the build script)
into `public/dist/`; `npm run gen` regenerates `client/` from the app's
OpenAPI document.

## What to look at

- **No fetch on load** — open the network panel: the todo list is there on
  first paint, seeded by `props.rs`. The component (`page.tsx`) is unaware;
  it just calls `useGetTodos(...)`.
- **Mutations reach the seed** — add or delete a todo and the seeded list
  refreshes, because `props.rs` seeds under the *same* canonical query key the
  hooks use (`["/api/todos", {...}]`).
- **One binary** serves the page, the bundle, the static CSS, the API, and
  `/openapi.json`. No Node at runtime.
- **Thin handlers** — `route.rs` files only map between the wire and
  `src/core/todos.rs`, which holds the logic.

See `docs/server-props.md` in the repo root for the design writeup.
