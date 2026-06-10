# React/TSX Support: page.tsx In The App Tree

**Status:** design manifest — no implementation yet. Follows PR #6 (typed React Query client), which is step one of this story.

## Context

The strategic bet: teams building on Next.js today will want to swap their backend for something faster and cheaper as they scale — without rewriting their frontend. nextrs should let them keep writing `.tsx` pages with the conventions they already know (file-based routes, layouts, loading states) while the server underneath is a single Rust binary.

PR #6 already gives React code a fully-typed line to the Rust backend (`#[nextrs::api]` → OpenAPI → orval → React Query hooks). This manifest covers the other half: **`.tsx` files as first-class pages in the `app/` tree.**

## Goal

```
app/
├── layout.rs           # Rust/Askama layouts keep working
├── page.rs             # Rust pages keep working
└── dashboard/
    ├── page.tsx        # React page — routed like page.rs
    ├── props.rs        # optional: Rust computes initial props per request
    └── loading.html    # existing streaming convention still applies
```

A `page.tsx` is discovered, routed, bundled, and served by the same build-time codegen that wires everything else. One Rust binary serves the APIs, the Rust pages, and the React pages — no monorepo split, no Node server.

## Non-Goals (for the first iterations)

- React Server Components / the RSC wire protocol.
- `layout.tsx` — Rust/Askama layouts wrap React pages for now (a React page is a child of the layout chain like any other page).
- Client-side navigation between pages (MPA semantics initially; each page load is a real request).
- A JS runtime inside the Rust binary (see rendering model — this is deliberately deferred).

## Rendering model — the core decision

Three ways to put React-rendered HTML in the response:

| Option | How | Verdict |
|---|---|---|
| **A. Client-side render (CSR)** | Server streams the layout shell + a mount div + script tag; React renders in the browser | **Phase 1.** Zero new runtime dependencies; the existing `loading.html` streaming covers perceived latency; data arrives via the PR #6 typed hooks |
| **B. Build-time prerender (SSG) + hydrate** | At build time, Node runs `renderToString` per `.tsx` page; the HTML ships in the binary like a static page; the browser hydrates | **Phase 2.** Node is already in the toolchain (orval); zero runtime JS engine; covers SEO/first-paint for request-independent content |
| **C. Runtime SSR** | Embed a JS engine (`deno_core`/V8, or QuickJS via `rquickjs`) and `renderToString` per request | **Phase 3 / research.** Real Next parity for request-dependent SSR, but a heavy dependency with Vercel cold-start cost — only justified once A+B prove demand |

A Node sidecar process for SSR is rejected outright: it breaks the single-binary story and can't run inside the Vercel Rust function.

The phasing matters more than the endpoint: **A ships value with the machinery (discovery, bundling, dev loop) that B and C reuse.** Most Next.js apps are dashboards whose pages are skeleton-then-data anyway — exactly what A + streaming + typed hooks reproduces.

## Data flow

Two complementary paths:

**1. Client fetching (exists after PR #6).** The page uses generated hooks; requests hit the same binary same-origin:

```tsx
// app/dashboard/page.tsx
import { useGetDashboard } from "@site/client";

export default function Dashboard() {
  const { data } = useGetDashboard();
  return <Stats data={data?.data} />;
}
```

**2. Server props (`props.rs`) — the Rust answer to `getServerSideProps`.** An optional sibling file computes initial data per request, server-side, with no client round-trip:

```rust
// app/dashboard/props.rs
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct DashboardProps { pub user: String, pub totals: Vec<u64> }

pub async fn props(req: http::Request<axum::body::Body>) -> DashboardProps {
    // middleware extensions, db calls, anything async
}
```

The framework serializes the result into the streamed HTML (`<script type="application/json" id="__nx_props__">…</script>`); the page component receives it as its initial props. Because the props type derives `ToSchema`, the **TypeScript type of the props object is generated through the same OpenAPI pipeline as the API client** — a field rename in `props.rs` breaks the `.tsx` compile. No waterfall, no hand-written types, works in the CSR phase without any SSR machinery.

`props.rs` composes with streaming naturally: the loading shell streams first, the props await happens server-side (same slot in the chunk sequence as a `page.rs` await), then the props JSON + mount script arrive.

## Conventions and codegen

- **Discovery** (`nextrs/src/discovery.rs`): the page slot learns `.tsx` (`page.{rs,html,tsx}`). Precedence: `page.rs` and `page.tsx` in the same segment is a `compile_error!`, same pattern as the route-GET conflict guard.
- **Codegen** (`nextrs/src/build.rs`): a `.tsx` page emits a generated shell handler — layout chain + `<div id="__nx_root__">` + props JSON (if `props.rs` exists) + `<script type="module" src="/dist/{route}.js">`. The handler is ordinary `PageFn`; streaming/middleware/layout composition need no changes.
- **Bundling**: esbuild, invoked by the build pipeline (`cargo build` shells out, like `style/build.sh` does for Tailwind). Entry point per `page.tsx`, `splitting: true` for shared chunks (one React copy), output to `site/public/dist/` — which means the existing static-asset story (ServeDir locally, `sync_public_dir` → CDN on Vercel) serves the bundles with zero new serving code. `public/dist/` is gitignored, generated.
- **Toolchain**: esbuild + react deps live in the same `site/client/` package PR #6 introduces — one `package.json`, one `npm install`.

## Dev server

`dev/main.rs` grows from "restart cargo on any change" to a two-track watcher:

- **Rust/template/content changes** → cargo rebuild + restart (today's behavior).
- **`.tsx` changes** → esbuild rebuild only (milliseconds), then touch the livereload trigger. No cargo rebuild, no server restart — the bundle is a static asset.

esbuild's incremental/watch API makes this nearly free. The watcher also reruns the orval client generation when `route.rs`/`props.rs` files change (closing the loop PR #6 currently leaves manual via `npm run gen`).

## Deployment implications

- **Vercel**: bundles ride the existing `public/` → CDN path; the function never serves JS. The shell pages are normal streamed responses. Nothing new.
- **Docker/serverful**: bundles are in `site/public/dist/`, already copied into the image and served by ServeDir. Nothing new.
- The esbuild step must run in both build environments (Vercel build container has Node; the Dockerfile builder stage adds it).

## Phases

1. **CSR pages** — discovery + codegen for `page.tsx`, esbuild bundling, shell handler, dev-server two-track watch. Demo: rebuild one demo route as `.tsx` using the PR #6 hooks.
2. **`props.rs`** — server props serialized into the stream, types through the OpenAPI pipeline.
3. **SSG prerender + hydration** — build-time `renderToString` for pages without request-dependent props.
4. **Runtime SSR (research)** — evaluate `deno_core` vs `rquickjs` for request-dependent SSR; only if real users need SEO on dynamic pages.

## Open questions

- **Props invalidation/refetch**: should `props.rs` data also be exposed as a generated query hook so the client can refetch it without a full reload?
- **Multiple React pages sharing state**: MPA semantics reset React state per navigation — acceptable initially, but client-side routing between `.tsx` siblings may become the most-requested feature.
- **`loading.html` vs React Suspense**: a `.tsx` page with its own Suspense boundaries may not want the framework's loading slot; probably "if `page.tsx` exists, `loading` is optional and they compose" — verify the UX.
- **Bundle size guardrails**: per-route entry + shared chunks is the right default, but we should emit a size report at build time before someone ships 2MB of vendor JS per route.
