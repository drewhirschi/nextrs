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

api/index.rs                Vercel serverless entry (+ x-cold instrumentation)
vercel.json                 Rust runtime declaration + catch-all rewrite
.cargo/config.toml          NEXTRS_SKIP_BUNDLE=1 on Vercel (see Deploy below)
public/dist/                prebuilt page.tsx bundle (committed; served on Vercel)
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

Run that command from the workspace root. If you invoke Cargo from inside
`examples/react-todos/`, its Vercel `.cargo/config.toml` is active; prefix the
local run with `NEXTRS_SKIP_BUNDLE=0` so the page bundle is regenerated.

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

## Deploy to Vercel

Deploying a nextrs **React** app (with the `tsx`/rolldown bundler) to Vercel has
several non-obvious requirements. They're all already wired up in this example —
this section explains *why*, so you don't have to rediscover them. The files
that make it work:

| File | Why it's needed |
|---|---|
| `api/index.rs` | The serverless entry: wraps the generated router in `StreamingVercelLayer`. |
| `vercel.json` | Declares the Rust runtime **explicitly** (`functions: { "api/index.rs": { "runtime": "vercel-rust@4.0.11" } }`) and the catch-all rewrite. Without the runtime line the build fails in setup. |
| `.cargo/config.toml` | Sets `[env] NEXTRS_SKIP_BUNDLE = "1"` so the build script **skips bundling on Vercel** (Vercel never runs `npm install` before `cargo build`, so rolldown would have no React to resolve). Also carries an empty `[build]` table — `vercel-rust` crashes (`Cannot read properties of undefined (reading 'target')`) on a `.cargo/config.toml` without one. Because cargo only reads this file when run *inside* this dir, local `cargo run` from the repo root still bundles normally. |
| `public/dist/` (committed) | Since bundling is skipped on Vercel, the **prebuilt** bundle is shipped as a static asset. It's deliberately **not** gitignored for this example so `vercel deploy` uploads it. Rebuild it with a release build before deploying (below). |
| `Cargo.toml` → `nextrs = { version = "0.2", … }` | Depends on the **published** crate (no path dep) so the example builds standalone from its own directory. A `[patch.crates-io]` in the repo-root `Cargo.toml` redirects to local source for development. |

### Steps

```sh
# 1. Regenerate the client + a fresh *minified* bundle (release profile minifies)
cd client && npm install && npm run gen && cd ..
cargo build --release -p react-todos          # rebuilds public/dist/ minified

# 2. Deploy from THIS directory (it builds standalone via the published crate)
vercel deploy --prod
```

### Gotchas worth remembering

- **First build is slow (~8–15 min)** — Vercel compiles rolldown + oxc (~50 crates) from scratch. It's cached after that, so incremental redeploys are ~40 s.
- **Function region is a project setting, not `vercel.json`.** `"regions": [...]` in `vercel.json` is ignored; set the region via the dashboard (Settings → Functions) or the API (`PATCH /v9/projects/{id}` with `{"serverlessFunctionRegion":"sfo1"}`), then redeploy.
- **The bundle must be rebuilt before deploy** if you changed `page.tsx` — the committed `public/dist/` is what Vercel serves (it won't re-bundle).
- **Cold-start instrumentation:** `api/index.rs` adds an `x-cold` / `x-init-ms` header on each response so cold vs warm can be measured (Vercel exposes no native signal). See `benchmarks/scripts/bench-cold.sh`.

See `docs/server-props.md` in the repo root for the design writeup.
