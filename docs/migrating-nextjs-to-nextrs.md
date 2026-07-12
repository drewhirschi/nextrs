# Migrating a Next.js App Router App to nextrs

A step-by-step procedure for converting an existing Next.js App Router app to
nextrs. Written to be followed by a coding agent: every step names the file to
create, the contract it must satisfy, and how to verify it. The reference
implementation for every pattern here is `examples/react-todos/` — when in
doubt, diff against it.

**The prime directive: the frontend stays identical.** The same React client
components, the same UI, the same flows. What changes is everything behind
them: the Node/Next server becomes one Rust binary that serves the pages, the
API, the OpenAPI document, and the static assets. Concretely:

| Stays (byte-identical or trivially diffable) | Rewritten (Rust) |
|---|---|
| Client components (`"use client"` pages and everything they import) | Server components → client component + `prefetch.rs` seed |
| Styles (Tailwind output, CSS files) | Route Handlers (`route.ts` → `route.rs`) |
| `public/` assets | `middleware.ts` / `proxy.ts` → `middleware.rs` |
| Third-party client packages (React Query, better-auth client, radix, …) | Data layer (kysely → sqlx), auth server, S3, image processing |
| zod-validated *shapes* (become serde structs with the same JSON wire shape) | Layouts (`layout.tsx` → `layout.html`/`layout.rs`) |
| Server-action *call sites* (components importing `app/actions/*`) | Server-action *modules* (`"use server"` bodies → fetch shim + `route.rs`, §6.2) |

> hhh-next note: sections marked **[hhh]** carry specifics for the
> `/home/drew/work/hhh-next` conversion (better-auth, kysely+postgres, S3,
> shadcn). Everything else is app-agnostic.

## 0. Mental model

Read these first if you haven't: the repo `README.md`, `MANIFEST.md`,
`docs/react-tsx-support.md`, `docs/server-props.md`,
`docs/typesafe-client-codegen.md`, and `examples/react-todos/README.md`
(especially its Deploy section).

The short version:

- **No JS runtime in the server, ever.** No SSR of React, no RSC. A `page.tsx`
  is client-rendered: the server streams a layout shell + `<div
  id="__nx_root__">` + a `<script type="module">` pointing at a prebuilt
  bundle. Rust pages (`page.rs` + Askama) are the "server components" of
  nextrs — but for a migration that keeps the frontend identical you will use
  `page.tsx` everywhere a page exists today.
- **The RSC data win is recovered by `prefetch.rs`**: a Rust sibling file that
  pre-runs API handlers on the server and streams their results into the HTML
  as a JSON script tag. The client loads them into the React Query cache
  before mount — first paint with data, zero client fetch, and the component
  cannot tell the difference (`docs/server-props.md`).
- **The API is typed end to end**: `route.rs` handlers annotated with
  `#[nextrs::api]` → OpenAPI document → orval → generated React Query hooks
  and TS types. A Rust field rename breaks the TSX compile.
- **Server actions become explicit endpoints behind a same-signature shim**
  (§6.2). Call sites keep importing the same module with the same functions;
  only the module body changes — from `"use server"` RPC to fetch calls. In
  action-heavy apps this, not `route.ts`, is most of the API conversion.
- **MPA navigation.** There is no client-side router. Every navigation is a
  real HTTP request and a fresh `QueryClient`. `next/link` becomes a plain
  anchor (via a shim, §4.4).
- **One binary** serves everything; on Vercel it's a single Rust serverless
  function behind a catch-all rewrite, with `public/` on the CDN.

## 1. Route inventory

Before writing any code, walk the Next.js `app/` tree and produce a worksheet
(`MIGRATION.md` in the target repo) with one row per route. For each, record:
URL, convention files present, server/client component status, data
dependencies (db tables, external services), auth requirements, and the nextrs
target files. If the app uses server actions, add a second table — one row per
`"use server"` module listing its exported functions, each function's input
schema, and its return/throw contract (§6.2). In action-heavy apps that table,
not the route list, is the real API surface.

### 1.1 Convention mapping

| Next.js | nextrs | Notes |
|---|---|---|
| `app/**/page.tsx` (client component) | `app/**/page.tsx` | Ports nearly unchanged (§4.2). Client-rendered, same as today. |
| `app/**/page.tsx` (server component) | `app/**/page.tsx` (client) + `app/**/prefetch.rs` + a `route.rs` API | The server half is rewritten in Rust (§4.3). |
| `app/**/layout.tsx` | `app/**/layout.html` or `layout.rs` + `layout.html` (Askama) | **`layout.tsx` is not a convention** — verified: discovery only picks up `.rs`/`.html` for the layout slot. Port the JSX to HTML (§5). |
| `app/**/loading.tsx` | `app/**/loading.html` (or `loading.rs`) | **`loading.tsx` is not a convention** (phase 3, unimplemented). Hand-render the skeleton JSX to static HTML — it's request-independent by definition, and it never hydrates (the slot is swapped out). |
| `app/api/**/route.ts` | `app/api/**/route.rs` | §6. |
| `middleware.ts` / `proxy.ts` (Next 16) | `app/middleware.rs` + nested `app/<seg>/middleware.rs` | No `matcher` config — scoping is by directory placement (§7). |
| `app/[param]/` | `app/[param]/` | Same directory convention. Becomes `/{param}` in Axum syntax. Verified for pages and API routes (`examples/react-todos/app/api/todos/[id]/`). |
| `app/[...all]/` (catch-all) | `{*all}` Axum wildcard | **Fixed in framework source 2026-06-11** (`discovery.rs::dir_name_to_segment` + macro `url_from_file` now map `[...x]` → `{*x}`); available in published `nextrs >= 0.2.1`. Projects pinned to `0.2.0` should use a git/path dep, upgrade, or keep the §10.2 enumeration workaround. |
| `app/(group)/` (route groups) | **Unsupported** | A `(group)` directory becomes a literal `(group)` URL segment. Flatten the tree; if two sibling groups had different layouts, give each subtree its own real segment or push the layout difference down. |
| `app/@slot/`, `(.)intercept` | **Unsupported** | No parallel/intercepting routes. Restructure as normal routes. |
| `app/error.tsx`, `global-error.tsx` | **No convention** | Use a client-side `ErrorBoundary` inside each `page.tsx` (the error component is a client component already — reuse it). Server-side failures: see §13 gaps. |
| `app/not-found.tsx` | **No convention** | The router 404s with an empty default. App-level workaround: add a custom fallback in `main.rs`/`api/index.rs` (§13). |
| `app/**/template.tsx`, `default.tsx` | **Unsupported** | Restructure. |
| Server Actions (`"use server"`) | `/api/actions/**` POST endpoints + same-signature client shim | §6.2. Call sites don't change; no backport. |
| `metadata` / `generateMetadata` | Static `<head>` in `layout.html`; per-page `document.title` in the client | See §13. |

### 1.2 Inventory commands

```sh
# All convention files
find app -type f \( -name "page.tsx" -o -name "layout.tsx" -o -name "loading.tsx" \
  -o -name "route.ts" -o -name "error.tsx" -o -name "not-found.tsx" \) | sort
# Server vs client pages: a page without "use client" at the top of its
# import graph is a server component
grep -rL '"use client"' app --include="page.tsx"
# Catch-alls and groups that need restructuring
find app -type d \( -name "\[...*" -o -name "(*)" -o -name "@*" \) 
# Server actions
grep -rn '"use server"' app src
```

**[hhh]** inventory result: ~23 pages (`/`, `/about`, `/pricing`, `/contact`,
`/auth/login`, `/auth/register`, `/app/**` ×4, `/admin/**` ×13), root +
`/app` + `/admin` layouts, 8 `loading.tsx`, `error.tsx`, `not-found.tsx`, two
API route files (`/api/auth/[...all]` — better-auth catch-all — and
`/api/avatar`), and `proxy.ts` (Next 16 middleware) doing session-based
redirects. No route groups, no parallel routes. The dominant surface is
server actions: 12 `"use server"` modules under `app/actions/` exporting 68
functions, imported by 16 client files — the conversion is mostly §6.2, not
the route table. Only 2 pages are server components *with data* (`/admin`,
`/admin/classes`); they're the only ones needing `prefetch.rs` seeds.

## 2. Project scaffold

Lay the nextrs app down next to (or replacing) the Next app. The layout,
mirrored from `examples/react-todos/`:

```
my-app/
├── Cargo.toml              # package + 3 [[bin]] targets
├── Cargo.lock              # COMMITTED (see §11.5)
├── build.rs                # emit_registry + emit_seeds + bundle_pages
├── rust-toolchain.toml     # pin rustc (rolldown/oxc needs ≥1.94)
├── vercel.json             # runtime pin + catch-all rewrite
├── askama.toml             # dirs = ["app"]
├── .cargo/config.toml      # NEXTRS_SKIP_BUNDLE on Vercel (§11)
├── api/index.rs            # Vercel serverless entry
├── app/                    # the convention tree (pages, props, routes, middleware)
├── src/
│   ├── lib.rs              # domain layer: db, auth, services
│   ├── main.rs             # local dev server
│   └── bin/dump-openapi.rs # writes client/openapi.json for orval
├── client/                 # npm package: generated hooks + page deps + shims
│   ├── package.json
│   ├── orval.config.ts
│   ├── tsconfig.json
│   └── src/
│       ├── index.ts            # barrel re-export of generated client
│       ├── nextrs-client.ts    # seed-hydration helper (copy from react-todos)
│       └── generated/          # orval output (do not edit)
└── public/                 # static assets; public/dist/ = committed page bundles
```

### 2.1 Cargo.toml

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
publish = false
default-run = "my-app"

[[bin]]
name = "my-app"
path = "src/main.rs"

[[bin]]
name = "dump-openapi"
path = "src/bin/dump-openapi.rs"

[[bin]]
name = "index"          # Vercel serverless entry
path = "api/index.rs"

[build-dependencies]
nextrs = { version = "0.3", features = ["build", "tsx"] }

[dependencies]
nextrs = { version = "0.3", features = ["vercel"] }
axum = "0.8"
tokio = { version = "1", features = ["full"] }
tower = "0.5"
vercel_runtime = { version = "2", features = ["axum"] }
askama = "0.15"
http = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
utoipa = "5"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
# plus your service deps: sqlx, aws-sdk-s3, … (§10)
```

Depend on the **published** `nextrs` crate (no path dep) so the app builds
standalone — that's what Vercel will do. If you're developing against a local
framework checkout, put a `[patch.crates-io]` in the *workspace root*
`Cargo.toml` (the pattern this repo uses for `examples/react-todos`):

```toml
[patch.crates-io]
nextrs = { path = "nextrs" }
nextrs-macros = { path = "nextrs-macros" }
```

### 2.2 build.rs

```rust
fn main() {
    // app/ tree → generated_registry() + generated_openapi()
    nextrs::build::emit_registry("app", "src/main.rs", "nextrs_routes.rs")
        .expect("emit_registry failed");
    // Typed seed companions for prefetch.rs
    nextrs::build::emit_seeds("app", "nextrs_seeds.rs").expect("emit_seeds failed");
    // Bundle page.tsx entries into public/dist/ with rolldown
    nextrs::bundle::bundle_pages(&nextrs::bundle::BundleConfig {
        app_dir: "app",
        client_dir: "client",
        client_alias: "@my-app/client",
        public_dist: "public/dist",
        // next/* shims + any tsconfig path aliases the pages use (§4.4)
        aliases: &[
            ("next/link", "src/next-shim/link.tsx"),
            ("next/navigation", "src/next-shim/navigation.ts"),
            ("next/image", "src/next-shim/image.tsx"),
        ],
        ..Default::default()
    })
    .expect("bundle_pages failed");
}
```

Notes, all verified in `nextrs/src/bundle.rs`:

- `@/*` → `client/src/*` is built in (shadcn-style imports resolve as long as
  the shared components live under `client/src/`). Extra `aliases` entries are
  `(pattern, replacement)` with the replacement **relative to `client_dir`**;
  exact patterns map to files, `x/*` patterns to directories. Mirror every
  alias in `client/tsconfig.json` `paths` so `tsc` agrees with the bundler.
  **Version caveat:** in published `nextrs 0.2.0` the `x/*` alias spelling
  (including the built-in `@/*`) silently never matches — the resolver's alias
  keys are prefix matches, not globs, so resolution falls through to tsconfig
  `paths` (the hhh conversion exploits exactly that fallback). Fixed in
  framework source 2026-06-11 (`bundle.rs::build_aliases` normalizes `X/*` →
  `X/` prefix form, and a user `@/*` entry now overrides the built-in); lands
  in `0.2.1` and later.
- Release builds (`cargo build --release`) minify and set
  `process.env.NODE_ENV = "production"`; debug builds don't.
- Bundle names are stable: `/dist/<slug>.js` where `/` → `index`, `/todos` →
  `todos`, `/users/{id}` → `users-_id_`, with shared chunks under
  `/dist/chunks/`. No content hashing.
- `NEXTRS_SKIP_BUNDLE=1` skips bundling entirely (used on Vercel and by the
  client-codegen bootstrap, §8).

### 2.3 main.rs (local dev), api/index.rs (Vercel), dump-openapi.rs

Copy these three from `examples/react-todos/` nearly verbatim
(`src/main.rs`, `api/index.rs`, `src/bin/dump-openapi.rs`), renaming the
crate. The essential shape:

```rust
// src/main.rs
include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

#[tokio::main]
async fn main() {
    let public_dir = std::env::var("NEXTRS_PUBLIC_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/public").to_string());
    let app = nextrs::router::build_router_with_public(generated_registry(), &public_dir)
        .merge(nextrs::openapi::spec_router(generated_openapi()));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

```rust
// api/index.rs
use nextrs::vercel::StreamingVercelLayer;
use tower::ServiceBuilder;
include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    let router = nextrs::router::build_router(generated_registry())
        .merge(nextrs::openapi::spec_router(generated_openapi()));
    let app = ServiceBuilder::new()
        .layer(StreamingVercelLayer::new())   // NOT vercel_runtime's VercelLayer —
        .service(router);                     // that one buffers text/html
    vercel_runtime::run(app).await
}
```

The react-todos `api/index.rs` additionally adds `x-cold`/`x-init-ms`/
`x-instance` response headers for cold-start measurement — copy that block too
if you'll benchmark.

Note: env vars. Rust does not auto-load `.env`. Either export them in your
shell for local dev or add `dotenvy::dotenv().ok();` at the top of `main()`
(dev binary only). On Vercel, set them in the project settings as usual. If
you support `NEXTRS_ENV_FILE`, make the watcher include that file; otherwise
watch `.env`.

Recommended local workflow for converted apps:

- Keep the Vercel `.cargo/config.toml` escape hatch (`NEXTRS_SKIP_BUNDLE=1`) for
  cloud builds that cannot run the Node bundler.
- Run local dev with `NEXTRS_SKIP_BUNDLE=0` so `page.tsx` bundles and route
  assets are regenerated.
- Put the watcher/helper in a tiny workspace package such as `xtask`, not as a
  binary in the main app package. Then `cargo dev` can compile the helper
  independently before it starts the real app build.
- Make `cargo dev` the watch-and-restart loop and `cargo dev-once` the single
  foreground server run. Watch Rust sources, `app/`, client sources and
  package files, and the env file the server loads.

Use `docs/local-dev-workflow.md` as the canonical template so converted apps do
not drift into slightly different `cargo dev` behavior.

### 2.4 The client package

`client/package.json` — copy from react-todos and add the page dependencies
(everything the Next app's *client components* import):

```jsonc
{
  "name": "@my-app/client",
  "private": true,
  "type": "module",
  "scripts": {
    "postinstall": "ln -sfn client/node_modules ../node_modules",
    "dump": "NEXTRS_SKIP_BUNDLE=1 cargo run --bin dump-openapi",
    "orval": "orval --config ./orval.config.ts",
    "gen": "npm run dump && npm run orval",
    "typecheck": "tsc --noEmit"
  },
  "dependencies": {
    "@tanstack/react-query": "^5.62.0",
    "react": "^19.0.0",
    "react-dom": "^19.0.0"
    // + the app's client deps: radix, lucide-react, clsx, better-auth, …
  },
  "devDependencies": {
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "orval": "^7.3.0",
    "typescript": "^5.7.0"
  }
}
```

The `postinstall` symlink matters: `page.tsx` files live under `app/`, and
module resolution for their bare imports (`react`, `@tanstack/react-query`)
walks up from `app/` — the symlink makes `client/node_modules` visible at the
app root. Keep it.

`client/tsconfig.json`: copy from react-todos; `include` must cover
`"../app/**/*.tsx"`, and `paths` must mirror the bundler aliases (the client
barrel, `@/*`, and the `next/*` shims).

Copy `client/src/nextrs-client.ts` from react-todos **unchanged** — it's the
seed-hydration helper the generated entry wrappers import. `client/src/index.ts`
is a barrel that re-exports `./generated/**` (update the export lines after the
first `npm run gen`).

### 2.5 Domain layer placement

`app/` convention files are wired in via mangled `#[path]` modules — **they
cannot import each other**. All shared logic (db access, auth, services) goes
in `src/` as the package's lib crate, and `route.rs`/`prefetch.rs`/`middleware.rs`
files call it by crate name (`my_app::core::bookings::list(...)`). Keep
handlers thin: extract → delegate to core → map to wire DTOs (the react-todos
`route.rs` files are the model). This isn't just style — fat handlers are
structurally unreachable from anywhere else.

## 3. Order of work

Convert in this order; each step leaves the app buildable:

1. Scaffold (§2) with an empty `app/` + one hello-world `page.tsx`; verify
   local run and `npm run gen` round-trip.
2. CSS pipeline + `public/` assets (§9).
3. Auth (§10.2) — the universal blocker: middleware, action-endpoint guards,
   and most pages depend on the session.
4. API routes (§6) and server-action modules → endpoints + shim (§6.2),
   module-by-module, read-only modules first; regenerate the typed client
   (§8) as annotated routes land.
5. Middleware (§7).
6. Layouts (§5), then pages leaf-first (§4), each with its `prefetch.rs` seed.
7. Deploy (§11), then the verification pass (§12).

## 4. Pages

### 4.1 What every page becomes

Every Next.js page — server or client component — becomes a **client-rendered
`page.tsx`** in nextrs. The difference is what happens to the server half:

- **Already a client component** (`"use client"`, data via hooks): port
  almost unchanged (§4.2).
- **Server component** (async, awaits db/fetch, returns JSX): split into a
  client component + a Rust data path (§4.3).

A `page.tsx` may not coexist with `page.rs`/`page.html` in the same segment
(compile error, verified), and `prefetch.rs` requires a `page.tsx` sibling
(compile error, verified).

### 4.2 Porting a client-component page

1. Copy the file. Drop nothing — `"use client"` directives are inert string
   literals in the bundle and may stay (keeps the source identical to the Next
   branch).
2. Imports of app-local modules: shared components/utils that the page imports
   move under `client/src/` and resolve via `@/*` (shadcn layout: components
   in `client/src/components/ui/`, utils in `client/src/lib/utils.ts` — the
   built-in alias handles `@/components/...`, `@/lib/...` as-is).
3. Imports of server-action modules (`@/app/actions/*` or similar) don't
   change either — the module *behind* the import becomes the fetch shim
   (§6.2). Never edit the call sites.
4. Imports of `next/*` resolve via shims (§4.4) — the import lines themselves
   don't change.
5. Data fetching: if the page already uses generated/typed hooks or plain
   fetch to `/api/...`, it keeps working same-origin. If it fetched through a
   Next-specific path, switch to the orval hooks (§8) — and backport that
   change to the Next branch if frontend identity is a hard requirement.
6. Route params: there is no `useParams()` from a framework router. The shim's
   `useParams`/`usePathname` derive from `window.location` (§4.4).

### 4.3 Converting a server-component page

Pattern. Given:

```tsx
// Next: app/admin/products/page.tsx (server component)
export default async function ProductsPage() {
  const products = await db.selectFrom("products").selectAll().execute();
  return <ProductTable products={products} />;
}
```

Produce three files:

**(a) The API endpoint** — `app/api/products/route.rs` (§6):

```rust
#[nextrs::api(get, operation_id = "getProducts",
    responses((status = 200, body = Vec<Product>)))]
pub async fn get() -> Json<Vec<Product>> {
    Json(my_app::core::products::list().await.into_iter().map(Into::into).collect())
}
```

**(b) The client page** — `app/admin/products/page.tsx`:

```tsx
import { useGetProducts } from "@my-app/client";
import { ProductTable } from "@/components/product-table";

export default function ProductsPage() {
  // Seeded from the stream by prefetch.rs: defined on first render, no fetch.
  const { data } = useGetProducts();
  return <ProductTable products={data?.data ?? []} />;
}
```

(orval's fetch client wraps responses in `{ data, status, headers }` — hence
`data?.data`. Keep the optional chain: deleting `prefetch.rs` must degrade to
fetch-on-mount, not crash.)

**(c) The seed** — `app/admin/products/prefetch.rs`. The exact contract:

```rust
include!(concat!(env!("OUT_DIR"), "/nextrs_seeds.rs")); // generated aliases

pub async fn prefetch(req: http::Request<axum::body::Body>) -> nextrs::QuerySeed {
    nextrs::QuerySeed::new()
        .seed(get_api_products(req.extensions()))
        .await
}
```

The signature is fixed: `pub async fn prefetch(http::Request<axum::body::Body>)
-> nextrs::QuerySeed`. It receives the **post-middleware** request, so
anything middleware stashed in `req.extensions()` (session, tenant) is
available. The generated shell handler awaits `props()` exactly where a
`page.rs` await would sit, then streams
`<script type="application/json" id="__nx_seeds__">…</script>` before the
mount div — so a `loading.html` still ships first.

**Seed keys.** Entries are keyed `[url]` (no params) or `[url, params]` —
orval's canonical format, so `invalidateQueries` and mutations reach seeded
data exactly like fetched data. The seed companion builds the key for you; the
one thing *you* must do is put `#[serde(skip_serializing_if = "Option::is_none")]`
on every `Option` field of a `Query` params struct — the client drops absent
keys when hashing, and a serialized `null` would make the Rust-built key never
match the hook's. (Verified: `nextrs/src/seed.rs`, react-todos `TodosFilter`.)

**Seed-companion eligibility** (verified in `nextrs-macros`): a companion
`get_<url_snake>` is generated only for an annotated `get` that takes **zero
args or exactly one `Query<T>` extractor** and returns `Json<T>`. Companion
names derive from the URL: `/api/todos` → `get_api_todos`, plus a module alias
`api_todos` for reaching the params type (`api_todos::TodosFilter`).

**Handlers that take `Path<…>` (or anything else) get no companion.** For a
page like `/admin/products/[id]` whose data endpoint is
`GET /api/products/{id}`, build the entry by hand — call core directly and
construct the key with `nextrs::seed_key`:

```rust
// app/admin/products/[id]/prefetch.rs
pub async fn prefetch(req: http::Request<axum::body::Body>) -> nextrs::QuerySeed {
    // No Path extractor here — parse the id from the URL.
    let id = req.uri().path().rsplit('/').next()
        .and_then(|s| s.parse::<i64>().ok());
    let Some(id) = id else { return nextrs::QuerySeed::new() };

    let product = my_app::core::products::get(id).await;
    let mut seed = nextrs::QuerySeed::new();
    if let Some(p) = product {
        seed = seed.seed(async move {
            nextrs::SeedEntry {
                key: nextrs::seed_key(&format!("/api/products/{id}"), None),
                data: nextrs::serde_json::to_value(WireProduct::from(p)).unwrap(),
            }
        }).await;
    }
    seed
}
```

Caveat, stated honestly: the manual path bypasses the handler, so if the
handler's wire shape drifts from what you serialize here, the page flickers
seed-shape → handler-shape on first refetch. Reuse the same wire DTO type the
`route.rs` uses (put shared DTOs in `src/` if needed). A key mismatch is safe
— it degrades to a refetch, never a break.

A page can seed several queries (`.seed(a).await.seed(b).await`). Seed what
the page reads on first paint; let the rest fetch on mount.

**searchParams:** the Next `searchParams` prop becomes `useSearchParams()`
from the shim (client-side) — and if a seeded query's params depend on them,
parse `req.uri().query()` in `prefetch.rs` (e.g. with `serde_urlencoded`) and
pass the same params struct to the companion so the keys line up.

### 4.4 `next/*` shims

Client components import `next/link`, `next/navigation`, `next/image`. Those
packages don't exist in the nextrs bundle. Keep the *source* unchanged and
redirect the imports at the bundler via `BundleConfig.aliases` (§2.2) to shim
files in `client/src/next-shim/`:

```tsx
// client/src/next-shim/link.tsx — MPA anchor with next/link's surface
import * as React from "react";
export default function Link({
  href, children, prefetch, replace, scroll, ...rest
}: React.AnchorHTMLAttributes<HTMLAnchorElement> & {
  href: string; prefetch?: boolean; replace?: boolean; scroll?: boolean;
}) {
  return <a href={href} {...rest}>{children}</a>;
}
```

```ts
// client/src/next-shim/navigation.ts
export function useRouter() {
  return {
    push: (url: string) => window.location.assign(url),
    replace: (url: string) => window.location.replace(url),
    back: () => window.history.back(),
    refresh: () => window.location.reload(),
    prefetch: (_url: string) => {},
  };
}
export function usePathname(): string {
  return window.location.pathname;
}
export function useSearchParams(): URLSearchParams {
  return new URLSearchParams(window.location.search);
}
export function useParams(): Record<string, string> {
  // No router: derive params where needed, or parse location.pathname
  // against the page's own known pattern at the call site.
  return {};
}
export function redirect(url: string): never {
  window.location.assign(url);
  throw new Error("NEXT_REDIRECT");
}
export function notFound(): never {
  // Next renders the not-found boundary client-side; MPA equivalent:
  // navigate to a real 404 page (a page.tsx rendering the old not-found UI).
  window.location.replace("/404");
  throw new Error("NEXT_NOT_FOUND");
}
```

```tsx
// client/src/next-shim/image.tsx — plain <img>, no optimization
import * as React from "react";
export default function Image({
  src, alt, width, height, fill, priority, quality, ...rest
}: any) {
  const style = fill
    ? { position: "absolute" as const, inset: 0, width: "100%", height: "100%", objectFit: "cover" as const }
    : undefined;
  return <img src={typeof src === "string" ? src : src?.src} alt={alt}
              width={width} height={height} style={style} {...rest} />;
}
```

Add the same three mappings to `tsconfig.json` `paths`. Behavior differences
are real and acceptable for MPA semantics: no prefetch, no soft navigation, no
image optimization (serve appropriately-sized originals from `public/`). If a
page uses `useParams()` for real work, replace the call with a
`usePathname()`-based parse and backport. If any page calls `notFound()`
client-side (**[hhh]** the admin `[id]` pages do), the `/404` page the shim
targets must exist — a `page.tsx` rendering the old `not-found.tsx` markup.

## 5. Layouts and loading

### 5.1 Layouts

`layout.tsx` must become `layout.html` (static) or `layout.rs` + Askama
template (dynamic). The root layout is where `<html>`, `<head>`, stylesheet
links, and fonts live:

```html
<!-- app/layout.html -->
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>My App</title>
  <link rel="stylesheet" href="/style.css">
  <link rel="icon" href="/favicon.ico">
</head>
<body>
  {{ children|safe }}
</body>
</html>
```

Rules (verified in `conventions.rs` / MANIFEST):

- Askama layouts (`layout.rs`) must use `{{ children|safe }}` — unescaped —
  or the framework's content marker gets HTML-escaped and the page never
  splices in. Static `layout.html` accepts `{{children}}` or `{{ children }}`
  (literal substitution).
- Layouts nest root-to-leaf, same as Next. A nested `layout.html` at
  `app/admin/layout.html` wraps every `/admin/**` page.
- An interactive layout (e.g. a navbar with a session-aware menu — **[hhh]**
  both the `/app` and `/admin` layouts) can't be a React layout. Two options:
  render the static frame in `layout.html` and mount the interactive part from
  the page bundle (move the navbar into a shared client component each page
  renders), or render it server-side with Askama from session data. Prefer the
  first — it keeps the React component identical.
- `app/` is also the Askama template dir (`askama.toml`: `dirs = ["app"]`), so
  `layout.rs` references `#[template(path = "layout.html")]` relative to `app/`.

### 5.2 Loading skeletons

For each `loading.tsx`, hand-render the skeleton to plain HTML and save it as
`loading.html` in the same segment. This is exact, not lossy: skeletons are
request-independent, and the slot is swapped out before React mounts, so
nothing hydrates. Tailwind classes carry over as-is (the CSS scan covers
`app/**/*.html`, §9).

With a `loading` slot present the route streams: layout-open + skeleton at
TTFB, then (after `props()` resolves) the seeds + mount div, then the swap
script + layout-close. Without one, the response is a single synchronous
chunk. Give every page whose `prefetch.rs` does real work a `loading.html`.

## 6. API routes: `route.ts` → `route.rs`

A `route.rs` exports `pub async fn get/post/put/patch/delete/head/options`.
Discovery is textual — the functions must be `pub` (or `pub(crate)`/
`pub(super)`) and `async`, declared exactly like that. Handlers are ordinary
Axum handlers: any extractors, any `IntoResponse`.

For the **typed client**, annotate with `#[nextrs::api]` and use concrete
shapes:

```rust
// app/api/bookings/route.rs
use axum::{Json, extract::Query};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Serialize, Deserialize, ToSchema)]
pub struct Booking { pub id: i64, pub class_name: String, pub starts_at: String }

#[derive(Serialize, Deserialize, IntoParams)]
pub struct BookingsFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct CreateBookingRequest { pub instance_id: i64 }

#[nextrs::api(get, operation_id = "getBookings", params(BookingsFilter),
    responses((status = 200, body = Vec<Booking>)))]
pub async fn get(Query(f): Query<BookingsFilter>) -> Json<Vec<Booking>> {
    Json(my_app::core::bookings::list(f.status.as_deref()).await
        .into_iter().map(Into::into).collect())
}

#[nextrs::api(post, operation_id = "createBooking",
    responses((status = 200, body = Booking)))]
pub async fn post(Json(req): Json<CreateBookingRequest>) -> Json<Booking> {
    Json(my_app::core::bookings::create(req.instance_id).await.into())
}
```

What the annotation does (verified in `nextrs-macros` and `build.rs`):

- `path` is derived from the file location (`[id]` → `{id}`) — never write it.
  (Raw `#[utoipa::path]` also works; the codegen then verifies your `path`
  against the file convention and fails the build on mismatch.)
- `operation_id` defaults to `getApiBookings`-style; override it for clean
  hook names (`getBookings` → `useGetBookings`). `tag` defaults to the last
  static segment and controls orval's file split.
- Request body is inferred from the `Json<T>` extractor. The response type is
  **not** inferred from the return — `responses((status = 200, body = T))` is
  required for a typed response.
- Annotation is opt-in per handler. Unannotated handlers still route; they
  just don't appear in the spec/client. The build writes a per-handler summary
  under `target/nextrs/`; set `NEXTRS_VERBOSE=1` when you want the same
  `client ✓` / `no client` lines echoed during codegen.

Other rules:

- **Path params:** `app/api/bookings/[id]/route.rs` + `Path(id): Path<i64>` +
  `params(("id" = i64, Path, …))` in the annotation. See
  `examples/react-todos/app/api/todos/[id]/route.rs`.
- **`page` owns GET**: a segment with a page may have a `route.rs` only if it
  doesn't export `get` (compile error otherwise, verified).
- **Status codes / errors:** Next's `NextResponse.json({error}, {status: 401})`
  becomes returning `(StatusCode, Json<ErrorBody>)` or an
  `impl IntoResponse`. If you need both a typed 200 body and error statuses,
  return `Result<Json<T>, (StatusCode, Json<ErrorBody>)>` and list both in
  `responses(...)`. Keep error JSON shapes identical to the Next app — clients
  may branch on them.
- **Cookies/headers:** there is no nextrs cookie API. Read with
  `axum::http::HeaderMap` / `req.headers()`; write with
  `(AppendHeaders([(SET_COOKIE, "…")]), Json(body))` or add `axum-extra = {
  features = ["cookie"] }` for `CookieJar`. `NextResponse.redirect(url)` →
  `axum::response::Redirect::to(url)`.
- **Raw bodies / multipart:** handlers are plain Axum — `axum::extract::Multipart`
  (enable the `multipart` feature) or `Bytes` work; they just won't be typed
  in the client (call them with plain `fetch`, same as the Next app did).

### 6.1 zod → serde mapping

| zod | Rust |
|---|---|
| `z.object({...})` | `#[derive(Serialize, Deserialize, ToSchema)] struct` (`IntoParams` instead of `ToSchema` for query structs) |
| `z.string()` | `String` |
| `.optional()` / `.nullish()` | `Option<T>` + `#[serde(skip_serializing_if = "Option::is_none")]` (mandatory on query/seed params, good hygiene elsewhere) |
| `z.number()` / `.int()` | `f64` / `i64` |
| `z.boolean()` | `bool` |
| `z.enum(["a","b"])` | `enum { A, B }` + `#[serde(rename_all = "lowercase")]` + `ToSchema` |
| `z.string().uuid()` | `uuid::Uuid` (utoipa `uuid` feature) |
| `z.string().datetime()` | `chrono::DateTime<Utc>` (utoipa `chrono` feature) — or keep `String` if the Next app passed ISO strings through |
| `z.array(T)` | `Vec<T>` |
| `.default(x)` | `#[serde(default)]` / `#[serde(default = "fn")]` |
| Refinements (`.min()`, `.email()`, …) | Not schema-level: validate in the handler (or the `validator` crate) and return the same 400 shape the Next app returned |
| `z.coerce.number()` | Query strings already coerce via `serde`'s deserializer for numeric types; for JSON bodies, accept the type the client actually sends |

The wire contract is the law: match the *JSON* the Next app produced/accepted
(field names, casing, null-vs-absent), not the zod source aesthetics.

### 6.2 Server actions: the same-signature fetch shim

In many App Router apps the real API surface isn't `route.ts` — it's
`"use server"` modules called like local functions from client components.
nextrs has no server-action RPC, but actions already *are* RPC: typed
functions, serialized args, serialized results. Replace the transport, not
the call sites. **[hhh]** is the proof case: 68 actions across 12 modules vs
2 route files.

Each `app/actions/<module>.ts` becomes two things:

1. **Rust endpoints**, one per exported function:
   `app/api/actions/<module>/<kebab-case-fn>/route.rs` exporting
   `pub async fn post` (e.g. `bookings.ts: createBooking` →
   `POST /api/actions/bookings/create-booking`).
2. **A client shim** with the same exported names and signatures, whose
   bodies are fetch calls to those endpoints. It lives wherever the existing
   import alias resolves — components importing `@/app/actions/bookings`
   resolve via the built-in `@/*` alias to
   `client/src/app/actions/bookings.ts`, so that's where the shim goes.

Importing components stay **byte-identical**: same import lines, same call
expressions, same promise results. No frontend change, no backport — this is
the difference from the orval-hook path (§8), which rewires call sites.

The convention, and why:

- **Everything is POST, reads included.** Next served every action as a POST
  under the hood, so idempotency/caching/CDN semantics carry over without
  auditing each function — and the `page`-owns-GET rule (§6) can never bite.
- **One directory per function**: a `route.rs` exports each method once, the
  path falls out of the file convention, and per-module guards fall out of
  directory-scoped middleware (§7) at `app/api/actions/<module>/middleware.rs`.
- **Body = the function's single argument**, JSON-encoded (object, scalar, or
  no body for zero-arg). Actions are overwhelmingly unary. For a multi-arg
  action, pack the args into one object *in the shim* — the exported
  signature keeps the original parameters, so call sites don't notice.
- Axum caps request bodies at 2 MB by default (`DefaultBodyLimit`); match or
  exceed the old `experimental.serverActions.bodySizeLimit` if it was raised.
- Next-server APIs inside action bodies (`revalidatePath`, `redirect`,
  `cookies()`) have no place in this pattern. `revalidatePath` is a no-op
  here — every page load re-fetches/re-seeds (MPA, §0) — so it drops; audit
  the others case by case. **[hhh]**: 4 `revalidatePath('/admin')` calls
  drop with no behavior change.
- `#[nextrs::api]` is **optional** here: the shim brings its own fetch, so
  the generated hooks go unused. Annotate if you want spec coverage and the
  build-time path check; raw handlers (like §10.2's) are also fine. Pick one
  policy per app.

The shared helper:

```ts
// client/src/app/actions/_fetch.ts
//
// The reviver must match ONLY the shapes the Rust wire layer emits for
// formerly-Date fields (`Date.toISOString()` form: ms + literal Z). A looser
// regex (e.g. allowing `+00:00` offsets or missing millis) silently revives
// timestamp strings that live INSIDE Postgres-built JSON columns — strings
// the old action RPC delivered as strings — and components break. Found the
// hard way in the hhh conversion; keep this exact.
const ISO = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/;
const reviveDates = (_k: string, v: unknown) =>
  typeof v === "string" && ISO.test(v) ? new Date(v) : v;

export async function action<T>(path: string, input?: unknown): Promise<T> {
  const res = await fetch(`/api/actions/${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: input === undefined ? undefined : JSON.stringify(input),
  });
  const text = await res.text();
  const body = text ? JSON.parse(text, reviveDates) : undefined;
  if (!res.ok) throw new Error(body?.error ?? `action ${path} failed (${res.status})`);
  return body as T;
}
```

A module shim (**[hhh]** example — `app/actions/bookings.ts` rewritten, same
exports):

```ts
// client/src/app/actions/bookings.ts
import { action } from "./_fetch";
import type { BookingWithDetails, BookingsFilter } from "@/lib/db-types";

export const getAllBookings = (filter?: BookingsFilter) =>
  action<BookingWithDetails[]>("bookings/get-all-bookings", filter);
export const checkInBooking = (id: string) =>
  action<BookingWithDetails>("bookings/check-in-booking", id);
// … one line per former action
```

And an endpoint (`ActionError { error: String }` is a shared wire DTO in `src/`):

```rust
// app/api/actions/bookings/check-in-booking/route.rs   [hhh] example
use axum::Json;
use http::StatusCode;

// Session + ADMIN role enforced by app/api/actions/middleware.rs and
// app/api/actions/bookings/middleware.rs (authz below).
#[nextrs::api(post, operation_id = "checkInBooking",
    responses((status = 200, body = BookingWithDetails)))]
pub async fn post(Json(id): Json<String>)
    -> Result<Json<BookingWithDetails>, (StatusCode, Json<ActionError>)>
{
    my_app::core::bookings::check_in(&id).await
        .map(|b| Json(b.into()))
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ActionError { error: e.to_string() })))
}
```

#### Contract preservation: the four drift hazards

Treat each action's *observable* contract — return shape, `Date`-ness, throw
behavior — as frozen. All four hazards below showed up in the first survey:

1. **Dates must come back as Dates.** Action RPC delivered real `Date`
   objects (the PG driver parses timestamps); plain JSON delivers ISO
   strings, and every `x.starts_at.toLocaleString()` in a component becomes
   a runtime error. The reviver above restores them pattern-wide. Verify
   against fixtures rather than trusting the regex: bare `DATE` columns may
   have arrived as strings or midnight `Date`s depending on the driver —
   match what components actually received. Rust side:
   `chrono::DateTime<Utc>` serializes to a form the reviver matches;
   `NaiveDate` deliberately doesn't (correct, if the old value was a bare
   date string).
2. **DECIMAL stays a string.** PG drivers return `NUMERIC` as strings and
   the UI tolerates (depends on) it. Don't "fix" it to `f64`: use `String`,
   or `rust_decimal::Decimal` with `#[serde(with = "rust_decimal::serde::str")]`.
   Exception: fields the old code explicitly cast (`::float`) are numbers —
   match per field. **[hhh]**: `price`, `amount`, `monthly_rate` are strings;
   revenue sums are floats.
3. **`null` vs `undefined` is a tri-state.** `JSON.stringify` drops
   `undefined` keys (absent) and keeps `null` — and update-style actions
   often mean "leave unchanged" vs "clear" by exactly that difference. Rust:
   `Option<Option<T>>` with `serde_with::rust::double_option` (missing →
   `None`, `null` → `Some(None)`, value → `Some(Some(v))`). **[hhh]**:
   `customer-products` `expiresAt` is exactly this.
4. **Throw vs result envelope, per module.** Some modules throw on failure
   (components catch + toast); others return an envelope —
   `ActionResult<T> = {success, data} | {success: false, errorKey, errorMessage}`
   — with stable, branched-on error keys. Map throws to non-2xx + `{error}`
   (the helper re-throws); map envelopes to **200 with the envelope** — a
   domain failure in an envelope module is not an HTTP error, and making it
   one breaks every `if (!result.success)` branch. **[hhh]**: `user-bookings`
   is the envelope module; its keys (`CLASS_FULL`, `BOOKING_LIMIT_REACHED`,
   …) are load-bearing.

Before calling a module done, fixture-diff it: capture each action's
JSON-visible output from the running Next app, then diff against the new
endpoint through the shim (`jq -S`, §12.2 style).

#### Validation moves to Rust — and only Rust

The old modules parsed `unknown` input with zod *inside the action*, i.e.
server-side. Deleting the zod schemas along with the action bodies therefore
loses nothing: the shim does **no validation**, and the Rust handler is the
only validator — exactly where validation lived before. Shapes come from the
serde derives (§6.1 table); refinements (`.min()`, enums, ranges) become
in-handler checks. Reproduce the failure surface components observe: if a
zod failure threw (→ toast of `error.message`), return non-2xx with an
`{error}` message matching what the UI showed. (Caution: production Next
masks server-action error messages — capture what components *actually* got
before promising message parity.)

#### Authz: add the guards the actions never had

Expect this finding, not as an edge case: server actions often perform **no
session checks of their own** — the only guard is page-level middleware in
front of the pages that call them, while the action endpoints are publicly
callable POSTs. **[hhh]**: all 68 actions are like this; several trust a
client-supplied `userId`.

The port must not reproduce that. Mandate explicit per-endpoint guards:

- `app/api/actions/middleware.rs` — require a session for everything under
  `/api/actions/**` (the session lookup from §10.2; stash the session in
  `req.extensions_mut()`).
- `app/api/actions/<module>/middleware.rs` — role checks for admin-only
  modules; mixed modules check per-handler from the stashed session.

This is a **deliberate hardening**, not parity. Verify it changes nothing
observable for legitimate flows: every legitimate caller is a component on a
page the old middleware already gated, so those requests carry the cookie
and pass untouched — the only difference is that direct unauthenticated or
wrong-role calls now get 401/403 instead of executing. Run the §12.1
logged-out/wrong-role checks against the action URLs themselves. Binding a
trusted `userId` *parameter* to the session instead of the client-supplied
value is a second, separate hardening — decide it explicitly per action
(**[hhh]**: the `user-bookings` functions take `userId` from the caller;
binding to the session is behavior-preserving for the `/app` pages, which
pass the session's own id).

#### Shim call vs seeded query: the boundary

Three data paths now exist; pick by what the component *was*:

- **Existing client component calling actions** → shim, always. Shim calls
  are plain promises (`useEffect` / event handlers), not React Query — seeds
  populate the RQ cache and are invisible to them.
- **Former server component** → new client page + annotated GET + orval hook
  + `prefetch.rs` seed (§4.3). That page is new code anyway, so hooks cost no
  frontend diff.
- **Both need the same data** → keep the POST action endpoint for the shim
  *and* add an annotated GET twin for the hook + seed: two thin handlers
  over one core fn and one wire DTO in `src/` (the shape-drift caveat of
  §4.3 applies). **[hhh]**: `/admin`'s `getDashboardStats` /
  `getUpcomingInstances` are called only by the former RSC page, so they
  become GET + seed with no POST twin.

## 7. Middleware

`middleware.rs` exports one function:

```rust
use nextrs::conventions::MiddlewareResult;

pub async fn handle(req: http::Request<axum::body::Body>) -> MiddlewareResult {
    // inspect req; either:
    MiddlewareResult::next(req)                       // continue (possibly mutated)
    // or:
    // MiddlewareResult::response(Redirect::to("/auth/login").into_response())
}
```

Verified semantics (`router.rs`): middleware composes **root-to-leaf**
(`app/middleware.rs`, then `app/admin/middleware.rs`, …) and runs before
layouts, loading streams, pages, `prefetch.rs`, and `route.rs` handlers — so
redirects get real status codes before any byte is streamed. A middleware may
mutate the request (insert the session into `req.extensions_mut()`) and pass
it on; downstream `prefetch.rs` and raw-request handlers see the extension.
If it reads the body, it must put a body back before `next(req)`.

There is **no matcher config**. Scoping is by placement:

- Next's `matcher: ['/app/:path*']` → put the file at `app/app/middleware.rs`.
- A global matcher with exclusions (the common "everything except static and
  /api/auth" pattern) → root `app/middleware.rs` plus early-return path checks
  inside `handle` for the exclusions. Static assets never hit middleware
  anyway (on Vercel the CDN serves them before the function; locally ServeDir
  is the router fallback — but note `/dist/*` and friends only bypass the
  router because no route matches them).

**[hhh]** `proxy.ts` becomes: root `app/middleware.rs` implementing the
landing-page and `/auth` redirects-when-authenticated, plus
`app/app/middleware.rs` (require session; admins → `/admin` unless
`?view=user`) and `app/admin/middleware.rs` (require session + ADMIN role).
Resolve the session **in-process** — call `my_app::core::auth::session_from_headers(req.headers())`
directly instead of the Next version's HTTP self-call to
`/api/auth/get-session` — and stash it in `req.extensions_mut()` so pages'
`prefetch.rs` and API handlers don't re-query.

## 8. The typed client (orval)

The pipeline (verified working in react-todos):

```
route.rs (#[nextrs::api]) ──build.rs──► generated_openapi()
   ├── served at /openapi.json (runtime)
   └── dump-openapi bin ──► client/openapi.json ──orval──► client/src/generated/**
```

Workflow:

```sh
cd client && npm install        # once; also creates the ../node_modules symlink
npm run gen                     # dump-openapi (with NEXTRS_SKIP_BUNDLE=1) + orval
# update client/src/index.ts to re-export the new generated modules
cd .. && cargo run              # cargo build now bundles pages against the fresh client
```

`NEXTRS_SKIP_BUNDLE=1` on the dump step breaks the chicken-and-egg: a new
`page.tsx` may import hooks that don't exist until orval runs, but orval needs
`cargo build` (for dump-openapi). Re-run `npm run gen` after **every**
`route.rs` signature/annotation change; CI should run `npm run typecheck` to
catch drift.

`orval.config.ts` (copy from react-todos): `client: "react-query"`,
`httpClient: "fetch"`, `baseUrl: "/"` (same origin — one binary), `mode:
"tags-split"`. The generated hooks call the same URLs with the same shapes the
frontend already used, which is what keeps the frontend identical: components
swap hand-rolled fetches for hooks once (backported), then never change again.

Scope note: the hooks serve pages you're rewriting anyway (former server
components, §4.3) and pages that fetched through Next-specific paths.
Components that call server-action modules keep doing so through the shim
(§6.2) and never touch the generated client — don't convert action calls to
hooks; that's a frontend change with no payoff.

How the frontend gets wired at runtime (no action needed, for understanding):
the bundler generates an entry wrapper per page that creates a `QueryClient`
(`staleTime: 30s` so seeds don't immediately refetch), calls
`seedQueryClient()` to load `#__nx_seeds__` into the cache (wrapping each
entry in orval's `{data, status, headers}` envelope), and mounts the page
under `QueryClientProvider` on `#__nx_root__`.

## 9. Styling and static assets

- **Tailwind**: nextrs has no CSS pipeline; you run the Tailwind CLI and
  commit/serve the output. Use the standalone Tailwind v4 CLI (no Node
  required at runtime — see `style/build.sh` in this repo for the
  self-downloading pattern):

  ```css
  /* style/input.css */
  @import "tailwindcss";
  @import "tw-animate-css";          /* [hhh] keep the app's existing imports */
  @source "../app/**/*.html";
  @source "../app/**/*.tsx";
  @source "../client/src/**/*.{ts,tsx}";
  /* + paste the app's @custom-variant / @theme blocks from its globals.css */
  ```

  ```sh
  style/build.sh        # → public/style.css ; root layout: <link rel="stylesheet" href="/style.css">
  ```

  Copy the Next app's `globals.css` content (theme variables, base styles)
  into `input.css` so the emitted utilities and tokens match. Rebuild whenever
  templates/pages gain new classes; commit the output (it's a static asset).
  PostCSS-specific plugins must be reproducible through the Tailwind CLI or
  pre-baked; **[hhh]** uses only `@tailwindcss/postcss` + `tw-animate-css`,
  both fine.

- **`public/`**: copy the Next app's `public/` verbatim — same root-URL paths.
  Locally ServeDir serves it as the router fallback; on Vercel the CDN serves
  it before the rewrite. One asymmetry, verified: locally routes win over
  same-named files; on Vercel static files win. Don't name a route after a file.

- **`public/dist/`** is generated by the bundler — but **committed**, because
  Vercel can't rebuild it (§11.3).

- **Fonts**: `next/font` doesn't exist. Self-host the font files in `public/`
  and add `@font-face` to `input.css`, or use a `<link>` in `layout.html`.
  Visual parity check required.

## 10. Backend service mapping

| Next.js dependency | Rust replacement | Notes |
|---|---|---|
| kysely + postgres-js / pg | **sqlx** (`features = ["runtime-tokio", "tls-rustls", "postgres", "chrono", "uuid"]`) | Same database, same schema, same queries. Translate query-builder calls to `sqlx::query_as!`/`query!` (compile-time-checked against the live schema via `DATABASE_URL`, or use `query_as::<_, T>` + `FromRow` to avoid the build-time DB dependency — prefer the latter for Vercel builds, or commit `.sqlx` offline data via `cargo sqlx prepare`). Lazily init a `PgPool` in a `tokio::sync::OnceCell` — don't pay for it at cold start before the first query, and keep `max_connections` small (≈5) per serverless instance; point at a pooled connection string (pgbouncer/Supabase pooler) if instance counts can spike. |
| better-auth (server) | Bun/Node sidecar at `/api/auth/*`, or implement its HTTP contract in Rust (§10.2) | Keep the better-auth **client** package unchanged either way. |
| @aws-sdk/client-s3 + s3-request-presigner | **aws-sdk-s3** (+ `aws-config`) | Presigning: `client.put_object().bucket(b).key(k).presigned(PresigningConfig::expires_in(Duration::from_secs(300))?)`. Custom endpoints (R2/MinIO) via `config::Builder::endpoint_url`. Reads/writes/deletes map 1:1. |
| sharp | **image** (decode/resize) + **webp** (lossy encode) + **kamadak-exif** (orientation) | `image`'s own WebP encoder is lossless-only; use the `webp` crate for `quality: 80`-style output. `sharp().rotate()` (EXIF auto-orient) is not automatic — read the EXIF orientation tag and apply `rotate90/180/270` before resizing. `resize(w, h, {fit: "cover"})` = scale to fill + center-crop (`resize_to_fill` in `image`). Bytes will differ from sharp's; behavior (dimensions, format, orientation) must not. |
| zod | serde + utoipa (+ manual validation) | §6.1. |
| NextResponse.json / .redirect / cookies | `axum::Json`, `(StatusCode, Json<T>)`, `Redirect::to`, `SET_COOKIE` headers / `axum-extra` `CookieJar` | §6. |
| next/headers `headers()`/`cookies()` | `req.headers()` in middleware/props; `HeaderMap`/`CookieJar` extractors in route.rs | |
| @vercel/og, next/og | Skip or pre-render | No equivalent; static OG images in `public/`. |

### 10.2 better-auth: keep the client; sidecar or reimplement the server

Either strategy below keeps the frontend identical: the `better-auth/react`
client package stays in `client/package.json` and the `auth-client.ts`
source ports unchanged. What differs is who answers `/api/auth/*`. Decide
this first — it's the riskiest call in the migration (a mis-step logs every
user out or locks them out).

**(a) Sidecar — the de-risked default.** Keep better-auth itself running as
a minimal Bun/Node service that owns `/api/auth/*` against the same Postgres
tables; everything else is Rust. Rust never re-speaks the protocol:
middleware and guards validate sessions by reading the `session` table
directly (cookie token → row → join user/role) — trivial, because
better-auth sessions are DB rows. Wire-parity risk (scrypt hashes, cookie
format and signing, OAuth flow, `get-session` payload shape) drops to ~zero:
same library, same tables. Costs: a second runtime — the one-binary story is
compromised until you later swap in (b) — plus its deploy story. Routing: on
Vercel, deploy the sidecar as a sibling JS function and put a
`/api/auth/(.*)` rewrite *before* the catch-all; on a single host, run it on
a loopback port and make each enumerated `/api/auth/*` `route.rs` a thin
forwarder (shared helper in `src/` preserving method, body, `cookie` and
`set-cookie` headers verbatim). Note server-side coupling: domain code that
calls better-auth's server API (**[hhh]** `auth.api.createUser` in
`createUserAndInvite`) calls the sidecar over HTTP under (a), or is
reimplemented under (b).

**(b) Reimplement in Rust — single binary, highest effort.** Rust implements
the same HTTP endpoints against the same Postgres tables (`user`, `session`,
`account`, `verification` — **[hhh]** with the snake_case field mappings
from `src/lib/auth.ts`). The rest of this section is the playbook for (b).

Recommendation: start with (a); capture wire fixtures (below) regardless, so
(b) remains a drop-in replacement once the rest of the migration has landed.
**[hhh]** did exactly this: (a) carried the migration through verification and
the first benchmarks, then slice 6 swapped in (b) — hand-rolled against the
fixtures — and deleted the sidecar.

**(c) `better-auth-rs` (crates.io `better-auth`) — watch this.** A community
Rust implementation of better-auth (Axum-first, plugin architecture, active
as of 2026-06). Its stable 0.x line generates its own schema (NOT a drop-in
for an existing TS-better-auth database), but its `v1` branch (alpha)
explicitly targets app-owned schema + full wire compatibility with
`better-auth@1.4.19`. Once v1 stabilizes it likely beats hand-rolling (b) —
but anyone adopting it for a migration must run it against captured fixtures
exactly as below; "compatible" is a claim, fixtures are a test. We chose (b)
over the alpha for the hhh conversion (decision 2026-06-12).

The `[...all]` catch-all route is unsupported (§1.1), so enumerate the
endpoints the client actually calls as explicit directories — the same list
serves as the sidecar's forwarder set under (a). **[hhh]** uses
`useSession`, `signIn.email`, `signIn.social` (Google), `signUp.email`,
`signOut`, and the proxy's `get-session` fetch — i.e.:

```
app/api/auth/get-session/route.rs        GET    → session+user JSON or null
app/api/auth/sign-in/email/route.rs      POST   {email, password} → set session cookie
app/api/auth/sign-up/email/route.rs      POST   {name, email, password}
app/api/auth/sign-out/route.rs           POST   → clear cookie
app/api/auth/sign-in/social/route.rs     POST   {provider, callbackURL} → {url} to redirect to
app/api/auth/callback/google/route.rs    GET    OAuth code exchange → set cookie, redirect
app/api/auth/forget-password/route.rs    POST   (check server flows too — [hhh]
                                                 createUserAndInvite fires it
                                                 server-to-self; in Rust call the
                                                 reset flow in-process instead)
app/api/auth/reset-password/route.rs     POST
```

Do **not** trust any document (including this one) for the exact wire shapes —
capture them from the running Next app and treat the captures as fixtures:

```sh
# Against the Next.js app, record exact request/response pairs + Set-Cookie:
curl -si -X POST localhost:3000/api/auth/sign-up/email \
  -H 'content-type: application/json' \
  -d '{"name":"T","email":"t@x.com","password":"hunter22"}'
curl -si localhost:3000/api/auth/get-session -H "cookie: $COOKIE"
curl -si -X POST localhost:3000/api/auth/sign-out -H "cookie: $COOKIE"
```

Implementation notes for the Rust side:

- **Session cookie**: replicate the exact cookie name (default
  `better-auth.session_token`; `__Secure-` prefix on HTTPS), value format
  (token + HMAC signature), and attributes (HttpOnly, SameSite=Lax, Path=/,
  expiry) as observed in the captures. Crates: `hmac` + `sha2` + `base64`.
  Sessions live in the `session` table — token lookup, expiry check, sliding
  refresh per the observed behavior.
- **Password hashing**: better-auth defaults to **scrypt**. Verify the stored
  format from a fixture row (register a user via the Next app, then make the
  Rust verifier accept that exact hash) before writing any login code. Crate:
  `scrypt` (or `password-hash`-compatible wrapper). Existing users must keep
  logging in: never re-hash, only verify.
- **Google OAuth**: standard code flow — build the consent URL
  (`sign-in/social` returns `{url}` for the client to navigate to), exchange
  the code in the callback, upsert `user`+`account`, set the session cookie,
  302 to the app. Crates: `oauth2` or hand-rolled with `reqwest`.
- Unannotated handlers are fine here — the better-auth client brings its own
  fetch layer; these endpoints don't need to be in the orval client. Leave
  them un-annotated (raw `impl IntoResponse` allowed) and exact-match the
  JSON.
- Last resort if (b)'s parity proves unreasonable: a minimal shared
  cookie-session auth implemented identically in both variants — a frontend
  change that must be backported to the Next branch. The sidecar option (a)
  should make this unnecessary.

### 10.3 **[hhh]** avatar route

`/api/avatar` GET/POST/DELETE: direct port. GET presigns a PUT (5 min,
`aws-sdk-s3`); POST fetches the temp object, EXIF-rotates, cover-resizes
256/64, encodes webp q80, writes `avatar.webp`/`avatar-thumb.webp`, deletes
temp, updates the user row (POST also accepts a legacy multipart form —
validate type and ≤4 MB); DELETE removes both objects and nulls `user.image`.
All three take an optional `userId` letting an **ADMIN** act on another user
(403 for non-admins) — port that on-behalf check explicitly. Session comes
from the request extensions (middleware) or a direct `core::auth` call; error
bodies are `{error}` with 400/401/403/413/500.

## 11. Build and deploy (Vercel)

All of these are load-bearing; each was discovered the hard way
(`examples/react-todos/README.md`). Deploy from the **app directory** (set the
Vercel project Root Directory accordingly).

### 11.1 `vercel.json`

```json
{
  "functions": {
    "api/index.rs": { "runtime": "vercel-rust@4.0.11" }
  },
  "rewrites": [
    { "source": "/(.*)", "destination": "/api/index" }
  ]
}
```

The explicit runtime pin is required — without the `functions` entry the build
fails during setup. Static files in `public/` are matched before the rewrite.

Two additions a *converted* repo (as opposed to a fresh nextrs app) needs,
both hit in the hhh deploy:

- **Framework detection hijacks the build.** The converted repo still has
  `"build": "next build"` in package.json (main branch needs it), so Vercel
  detects Next.js and runs it — on the Rust branch that build fails or, worse,
  deploys the wrong thing. Pin it off in `vercel.json`:
  `"framework": null, "buildCommand": "echo functions-only", "installCommand": "bun install"`
  (keep the install — the sidecar function's dependency tracing needs
  node_modules).
- **`.vercelignore` is mandatory.** The CLI ignores `node_modules` but NOT
  `target/` (tens of thousands of files → "files should NOT have more than
  15000 items") or the standalone tailwind binary `style/tailwindcss`
  (>100 MB → hard upload error). Minimum:
  `target`, `.next`, `node_modules`, `style/tailwindcss`.
- **Reason-less `BLOCKED` deployments: check your commit email.** hhh
  deployments stuck in `BLOCKED` (no error via API, no build events) turned
  out to be Vercel's git-author check: the branch's commit email didn't match
  a GitHub account linked to the Vercel user. The dashboard shows the reason;
  the API doesn't. Fix: rewrite the branch's author/committer emails
  (`git filter-branch --env-filter` over `<base>..<branch>`) and set the
  repo's `user.email` so future commits match. Enforcement was intermittent
  for us — don't let one successful deploy rule this out.
- **Escape hatch: prebuilt deploys.** If cloud builds are unavailable or you
  want second-fast deploys, build locally and upload only the output:
  `vercel pull && vercel build --prod && vercel deploy --prebuilt --prod`.
  The local `vercel-rust` builder cross-compiles for Lambda's older glibc via
  **`cargo-zigbuild`** — install `zig` (mise works) and
  `cargo install cargo-zigbuild` first, or the Rust function is silently
  missing from `.vercel/output`. Upload drops from full-source to ~12 MB and
  deploys in seconds.

### 11.2 `rust-toolchain.toml`

```toml
[toolchain]
channel = "1.96.0"
```

Vercel's Rust image ships an older compiler; rolldown→oxc needs ≥ 1.94.
Without the pin: `rustc … is not supported by oxc`.

### 11.3 `.cargo/config.toml` + committed `public/dist/`

```toml
[env]
NEXTRS_SKIP_BUNDLE = "1"

# Empty but REQUIRED: vercel-rust reads config.build.target and crashes
# ("Cannot read properties of undefined (reading 'target')") if a
# .cargo/config.toml exists without a [build] table.
[build]
```

Vercel never runs `npm install`, so rolldown would have nothing to resolve
React from — the build script must skip bundling there. Cargo only reads this
config when invoked *inside* the app dir (which is what Vercel does), so local
builds from a workspace root still bundle normally. **If you develop by
running cargo from inside the app dir, this env will silently skip bundling
for you too** — run from the workspace root, or unset the var.

Consequence: the prebuilt bundle is what Vercel serves, so `public/dist/` is
**committed, not gitignored**, and must be rebuilt with a **release** build
(minified, NODE_ENV=production) before every deploy that touched any page or
the client.

### 11.4 Deploy steps

```sh
cd client && npm install && npm run gen && cd ..
cargo build --release            # rebuilds public/dist/ minified
git add public/dist && git commit ...
vercel deploy --prod             # from the app directory
```

### 11.5 Standalone `Cargo.lock` (commit it)

The app's own `Cargo.lock` is committed (see
`examples/react-todos/Cargo.lock`). When the app builds standalone on Vercel —
outside any workspace — a missing lockfile means Vercel resolves dependencies
fresh from crates.io on every build, so an upstream semver-compatible release
(of rolldown/oxc/anything) can break the build with no change on your side.
The pinned lockfile makes Vercel builds reproducible. Refresh it deliberately
(`cargo update` locally, build, test, commit). Note: if the app lives inside a
dev workspace, cargo normally keeps one lock at the workspace root — generate
the standalone one by running `cargo generate-lockfile`/`cargo build` *in the
app dir* (temporarily outside the workspace, or with the workspace excluded)
and commit it.

### 11.6 Remaining gotchas

- **First Vercel build is slow (~8–15 min)** — it compiles rolldown + oxc
  (~50 crates) even though bundling is skipped (they're build-deps). Cached
  afterward (~40 s incremental).
- **Function region is a project setting, not `vercel.json`** — `"regions"`
  there is ignored for this runtime. Set it in Dashboard → Settings →
  Functions, or `PATCH /v9/projects/{id}` with
  `{"serverlessFunctionRegion":"sfo1"}`, then redeploy. Benchmark fairness
  and DB latency both depend on this.
- **`x-cold` instrumentation**: copy the header block from react-todos
  `api/index.rs` if you need cold/warm visibility — Vercel exposes no signal.
- In-process state does not survive cold starts and isn't shared across
  instances — anything the Next app kept in module scope (caches, rate-limit
  counters) needs the database or to be dropped.
- **DB migrations need a new home** if the old build ran them (**[hhh]**:
  `buildCommand` = migrate-then-build). Run them as a deploy step, or at
  startup behind an advisory lock — never unguarded from serverless cold
  starts (concurrent instances race). The Rust runner must be compatible with
  the existing bookkeeping table (same ids in e.g. `schema_migrations`) so
  production doesn't re-apply old migrations.

## 12. Verification

### 12.1 Per-route checklist

For every route, against both the old app and the new one (same DB):

- [ ] `GET` returns 200; HTML contains the layout, the stylesheet link, and
      (for tsx pages) `#__nx_root__` + the `/dist/<slug>.js` script tag.
- [ ] First paint has data with **no client fetch** for seeded queries
      (network panel: no request to the seeded endpoint on load; verify
      `#__nx_seeds__` content matches a direct `curl` of the endpoint).
- [ ] Loading skeleton streams first on routes that have one
      (`curl --no-buffer` shows ≥2 chunks separated in time).
- [ ] Logged-out / logged-in / wrong-role each produce the same status code
      and `Location` as the old app (compare `curl -si` outputs).
- [ ] Dynamic segments: valid id renders; bogus id produces the same
      error behavior as before.
- [ ] Every mutation on the page works and refreshes the visible data
      (invalidation reaches the seeded entry — add/edit/delete round-trip).
- [ ] No console errors; no 404s for chunks/assets.

### 12.2 API parity

For every endpoint: `curl` old vs new with identical inputs (including auth
cookie) and diff status, headers that matter (`set-cookie`, `location`,
`content-type`), and JSON bodies (`jq -S` to normalize). Error cases too
(401/403/404/400) — frontends branch on these.

### 12.3 Overall parity checklist

- [ ] Client component sources are identical (or trivially diffable) between
      branches: `diff -r` the page/component trees; every intentional
      difference is a documented backport.
- [ ] Auth: register → login → session persists across requests → logout, in
      both; the same session cookie semantics (a cookie minted by one app is
      ideally honored by the other — the strongest possible parity test).
- [ ] Styles: spot-check pages side by side; computed styles for key
      components match (Tailwind output parity).
- [ ] `public/` assets byte-identical at the same URLs.
- [ ] `npm run typecheck` clean in `client/`.
- [ ] `target/nextrs/*.client-summary.txt` reviewed — every handler that should
      be in the client says `client ✓`.
- [ ] Deployed: streaming verified on Vercel (`curl --no-buffer`), static
      assets show `x-vercel-cache: HIT`, region as intended.

## 13. Framework gaps (verified against `nextrs` source)

Things Next.js does that nextrs currently cannot. Each verified by reading
`nextrs/src/{discovery,build,router,bundle}.rs` and `nextrs-macros` — not
guessed. Gaps marked **framework-first** should be fixed in nextrs before (or
during) a conversion that needs them.

| Gap | Evidence | Workaround |
|---|---|---|
| **Catch-all segments `[...x]`** | `discovery.rs::dir_name_to_segment` only handles `[x]` → `{x}`; `[...all]` emits `{...all}`, which Axum rejects at router build | Enumerate concrete endpoint dirs (works for better-auth, §10.2). **Framework-first** if true wildcards are needed: map `[...x]` → `{*x}`. |
| **`layout.tsx` / `loading.tsx`** | `discovery.rs` sets `tsx: None` for both slots; react-tsx doc phase 3 unimplemented | Hand-port to `layout.html`/`.rs` and `loading.html` (§5). Exact for skeletons; interactive layouts need restructuring. |
| **`error.tsx` / error boundaries for server failures** | README "Not yet"; with a loading slot the 200 + headers are committed before `props()`/page run, so a late failure can't change the status | Client `ErrorBoundary` for render errors; make `props()` infallible-ish (seed nothing on error → page falls back to fetch-on-mount, which surfaces errors through the hooks); guard auth/404-able conditions in middleware, *before* streaming. |
| **`not-found.tsx` / custom 404** | Router has no 404 convention; fallback is ServeDir (or default 404) | App-level: wrap the public dir fallback — `ServeDir::new(dir).not_found_service(your_404_handler)` in `main.rs`/`api/index.rs`. On Vercel, also ensure the function (not the CDN) answers misses — it does, via the catch-all rewrite. |
| **Route groups `(g)`, parallel `@slot`, intercepting routes** | No special-casing anywhere in discovery | Flatten/restructure the tree. |
| **Server Actions** | No concept; POST handlers are `route.rs` | Same-signature fetch shim over `/api/actions/**` endpoints (§6.2) — call sites unchanged, nothing to backport. |
| **Metadata API (`metadata`, `generateMetadata`)** | Nothing reads page metadata; the tsx shell is a fixed mount div | Static `<head>` per layout segment (`layout.html` nests, so `/admin/**` can have its own `<title>`); truly per-page/dynamic titles via `document.title` in the page (SEO-relevant pages: consider a `layout.rs` that derives title from the path). |
| **SSR/SEO for React pages** | Hard constraint: no JS runtime in the server (`react-tsx-support.md`) | Content that must be in initial HTML for SEO belongs in a Rust page (`page.rs` + Askama) — a real frontend rewrite for that route, or accept CSR. The seeds JSON *is* in the initial HTML but isn't rendered markup. |
| **Client-side navigation / prefetch** | Documented non-goal (MPA semantics); fresh `QueryClient` per page load | Accept full-page navigations (`next/link` shim = `<a>`); cross-page cache invalidation is replaced by re-seeding server-fresh data each load. |
| **`next/image` optimization** | No image endpoint | `<img>` shim (§4.4); pre-size assets; the avatar pipeline (§10.3) covers user uploads. |
| **Mode-1 typed page props (`usePageProps`)** | `build.rs::emit_page_slot` only accepts `props()` → `QuerySeed` and calls `.to_script_tag()`; no bespoke-props injection, no `nextrs/client` helper exists | Model page-shaped data (session, flags) as a small GET endpoint and seed it like everything else. **Framework-first** if that's too contorted. |
| **Seed companions limited to zero-arg / single-`Query` GET returning `Json<T>`** | `nextrs-macros::seed_companion` match arms; `build.rs::get_is_seed_eligible` | Path-param or multi-extractor handlers: build `SeedEntry` manually with `nextrs::seed_key` (§4.3), reusing the handler's wire DTO to avoid shape drift. |
| **No cookie/session API surface** | Nothing in `conventions.rs`/`lib.rs`; handlers get raw `http::Request`/Axum extractors | `axum-extra` `CookieJar`, or hand-roll on headers (§6, §10.2). |
| **Middleware matchers** | `router.rs` scopes purely by path prefix of the file's segment | Directory placement + in-handler path checks (§7). |
| **`page` + `route.rs::get` on one path** | Codegen `compile_error!` (page owns GET) | Move the API GET to a sibling `app/api/...` path (it almost always already is). |
| **ISR / revalidate / fetch cache / draft mode** | Everything is dynamic per-request | Usually a non-issue (it's faster); for genuinely static heavy pages, put a CDN cache-control header on the response from the handler. |
| **One streaming boundary per route** | Roadmap item (no nested Suspense-style boundaries); single `loading` slot | Per-section spinners stay what they already are: client-side React Query loading states. |

### 13.1 App-side risks (not framework gaps, but they recur)

These come from the apps being migrated, not from nextrs — the first survey
(hhh) hit every one of them. Ranked roughly by blast radius:

- **Auth server parity** (§10.2). The single riskiest decision; default to
  the sidecar.
- **Server-action serialization drift** (§6.2): Dates, DECIMAL strings,
  null/undefined tri-state, throw-vs-envelope. Surfaces as subtle UI breakage
  spread across every consuming component; fixture-diff per module.
- **Timezone semantics.** JS date math (`new Date()`, `getDay()`, week
  bucketing, "next N days" windows) runs in the server process's local TZ;
  chrono in Rust defaults to UTC. A naive port silently shifts times and
  buckets near midnight and week boundaries. Make the TZ explicit: read an
  `APP_TIMEZONE` env at startup (chrono-tz), set to whatever the old
  deployment effectively ran in, and run the ported date-math tests in that
  TZ first. **[hhh]**: an `APP_TIMEZONE` env exists but is dead code — the
  real behavior is server-local time in week generation and the 7-day
  booking window.
- **Port logic engines behind the old tests.** Where the old app has a real
  test suite over its business logic, that suite is the spec: keep it green
  against the same Postgres while porting, and either port the cases to Rust
  or point the TS tests at the Rust endpoints where shapes allow. **[hhh]**:
  the booking/credit engine (limit bucketing per_week/per_billing_period,
  credit FIFO, reactivation, refunds; ~650 lines with bun + PG integration
  tests) ports last among the action modules, behind those tests.
- **Latent concurrency bugs surface in the port.** Check-then-insert without
  a transaction, locks held outside any transaction — Rust+sqlx makes real
  transactions cheap, and fixing these is right, but it's a behavior change
  under race: decide preserve-vs-fix explicitly per flow. **[hhh]**:
  non-transactional `bookClass` capacity check; a `FOR UPDATE` outside any
  transaction in credit deduction.

## 14. Time tracking

(Per the migration plan.) Log each slice's wall-clock and agent time in
`docs/hhh-migration-timelog.md`, with a note whenever something took
disproportionately long — those notes feed back into this guide.
