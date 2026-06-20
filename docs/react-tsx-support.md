# React/TSX Support: page.tsx In The App Tree

**Status:** phases 1 and 2 implemented (CSR `page.tsx` via rolldown bundling; `props.rs` seeding the React Query cache). The runnable `examples/react-todos` crate exercises the full pipeline. Phase 3 (build-time prerender, `loading.tsx`) not started.

One toolchain change from the original: the bundler is **rolldown** (all-Rust, the Vite 8 engine, on crates.io since May 2026), not `swc_bundler` — swc's bundling is officially slated for removal and lacks code splitting.

## Context

The strategic bet: teams building on Next.js today will want to swap their backend for something faster and cheaper as they scale — without rewriting their frontend. nextrs should let them keep writing `.tsx` pages with the conventions they already know (file-based routes, layouts, loading states) while the server underneath is a single Rust binary.

PR #6 already gives React code a fully-typed line to the Rust backend (`#[nextrs::api]` → OpenAPI → orval → React Query hooks). This manifest covers the other half: **`.tsx` files as first-class pages in the `app/` tree.**

## Goal

```
app/
├── layout.rs           # Rust/Askama layouts keep working
├── page.rs             # Rust pages keep working
└── dashboard/
    ├── page.tsx        # React page — routed like page.rs, client-rendered
    └── loading.tsx     # React loading skeleton — prerendered at build time
```

A `page.tsx` is discovered, routed, bundled, and served by the same build-time codegen that wires everything else. One Rust binary serves the APIs, the Rust pages, and the React pages — no monorepo split, no Node server.

## Hard constraint: no JS runtime in the Rust binary

Decided in review: **runtime server-side JS execution is out, permanently — not deferred.** No embedded V8/`deno_core`/QuickJS, no Node sidecar. Consequences, stated plainly:

- Executing a `.tsx` component *is* executing JavaScript. Anything that needs a component rendered per-request on the server cannot exist under this constraint. That includes classic SSR and real React Server Components.
- The framework's answer for request-time server rendering is the one it already has: **write `page.rs`.** Rust pages are the "server components" of nextrs.
- What `.tsx` gets instead: client rendering by default, plus **build-time** prerendering where Node runs once during `cargo build` (Node is already in the toolchain for orval) and ships static HTML into the binary. Build-time Node is a build dependency like the Tailwind CLI; runtime stays pure Rust.

### The RSC question

Can we get what RSC gives without a JS runtime? RSC's practical wins are (1) fetch data on the server before the component renders — no client waterfall — and (2) ship less JS. Win 2 is out of reach for `.tsx` without server execution. Win 1 has a Rust-native path: **the server fetches the data (Rust, per request) and injects it into the streamed HTML as JSON; the client component picks it up as initial data.** Same chunk sequence as a `page.rs` await, no extra round-trip, and the payload's TypeScript type is generated through the existing OpenAPI pipeline so Rust renames break the `.tsx` compile. Explored in depth in **`docs/server-props.md`** — including why the no-JS-runtime constraint makes a Rust sibling file (`props.rs`) the only possible shape for the server half, and a mode that seeds the React Query cache so components keep authoring against plain hooks.

## Non-Goals

- Runtime JS execution in the server (see above — permanent).
- React Server Components / the RSC wire protocol.
- `layout.tsx` — Rust/Askama layouts wrap React pages for now.
- Client-side navigation between pages (MPA semantics initially; each page load is a real request).

## Rendering model

| Convention | How it renders | Phase |
|---|---|---|
| `page.tsx` | **Client-rendered (the default for `.tsx`).** Server streams the layout shell + mount div + script tag; React renders in the browser; data via the generated typed hooks | 1 |
| `loading.tsx` | **Prerendered at build time** to static HTML (Node `renderToString` during `cargo build`), then used exactly like `loading.html`. Loading skeletons are request-independent by definition, so build-time rendering is exact — and no hydration is needed since the slot gets swapped out | 2 |
| `page.tsx` (static) | Optional build-time prerender + hydrate for pages whose initial markup doesn't depend on the request — SEO/first-paint without runtime JS | 2 |

Phase 1 ships value with the machinery (discovery, bundling, dev loop) that phase 2 reuses. Most Next.js apps are dashboards whose pages are skeleton-then-data anyway — exactly what CSR + the existing streaming + typed hooks reproduces.

## Data flow

The page uses the generated hooks from PR #6; requests hit the same binary same-origin:

```tsx
// app/dashboard/page.tsx
import { useGetDashboard } from "@site/client";

export default function Dashboard() {
  const { data } = useGetDashboard();
  return <Stats data={data?.data} />;
}
```

Server-fetched initial data (avoiding the mount-then-fetch round-trip) is designed in `docs/server-props.md` and slots in as phase 1.5.

## Conventions and codegen

- **Discovery** (`nextrs/src/discovery.rs`): the page slot learns `.tsx` (`page.{rs,html,tsx}`), the loading slot learns `loading.tsx`. Precedence: `page.rs` and `page.tsx` in the same segment is a `compile_error!`, same pattern as the route-GET conflict guard.
- **Codegen** (`nextrs/src/build.rs`): a `.tsx` page emits a generated shell handler — layout chain + `<div id="__nx_root__">` + `<script type="module" src="/dist/{route}.js">`. The handler is an ordinary `PageFn`; streaming/middleware/layout composition need no changes.
- **Bundling — Rust toolchain (swc), not esbuild.** Decided in review: keep the toolchain Rust. The swc crates (`swc_ecma_*` for the TSX transform, `swc_bundler` for bundling, `swc_ecma_minifier` for release) run **inside the build pipeline as a library** — `cargo build` does everything, no shell-out, no Node for the bundling path. Deno's bundler shipped on `swc_bundler`, so the path is proven, but two honest risks to validate in a spike before committing:
  1. `node_modules` resolution for react/react-dom (swc_bundler needs a resolver implementation; deno wrote one, so can we).
  2. Shared-chunk splitting across routes — `swc_bundler`'s splitting is weaker than esbuild's. Acceptable fallback for v1: self-contained per-route bundles (one React copy per route, fine at demo scale), with rolldown/esbuild reconsidered only if this becomes a real cost.
- Output goes to `site/public/dist/` (gitignored, generated) — the existing static-asset story (ServeDir locally, `sync_public_dir` → CDN on Vercel) serves the bundles with zero new serving code.
- **Toolchain**: react deps live in the same `site/client/` package PR #6 introduced — one `package.json`, one `npm install`. Node remains build-time-only (orval today; prerendering in phase 2).

## Dev server

The `xtask` watcher grows from "restart cargo on any change" to a two-track watcher:

- **Rust/template/content changes** → cargo rebuild + restart (today's behavior).
- **`.tsx` changes** → re-run the swc bundling step only (milliseconds, in-process), then touch the livereload trigger. No server restart — bundles are static assets.

The watcher also reruns the orval client generation when `route.rs` files change (closing the loop PR #6 currently leaves manual via `npm run gen`).

## Deployment implications

- **Vercel**: bundles ride the existing `public/` → CDN path; the function never serves JS. The shell pages are normal streamed responses. Nothing new.
- **Docker/serverful**: bundles are in `site/public/dist/`, already copied into the image and served by ServeDir. Nothing new.
- Because bundling is a cargo-side library call, both build environments get it for free; Node is only needed where phase 2's prerender runs.

## Phases

1. **CSR pages** — discovery + codegen for `page.tsx`, swc-based bundling inside the build pipeline (spike `swc_bundler` resolution/splitting first), shell handler, dev-server two-track watch. Demo: rebuild one demo route as `.tsx` using the PR #6 hooks.
2. **Server props (`props.rs`)** — Rust-fetched initial data injected as typed JSON into the stream; see `docs/server-props.md` (mode 1: typed initial props; mode 2: React Query cache seeding).
3. **Build-time prerender** — `loading.tsx`, plus optional prerender+hydrate for request-independent `page.tsx`. Node at build time only.
4. **Research** (no commitment): client-side navigation between `.tsx` siblings; `layout.tsx`.

## Open questions

- **swc_bundler viability** — the phase-1 spike: resolve react from `node_modules`, bundle a hook-using page, measure output size vs esbuild. If it fails badly, revisit the toolchain decision with data.
- **`loading.tsx` and Suspense**: a `.tsx` page with its own Suspense boundaries may not want the framework's loading slot; probably "they compose" — verify the UX.
- **Bundle size guardrails**: emit a size report at build time before someone ships 2MB of vendor JS per route.
- **Initial-data injection shape** (research item 3): per-route Rust data fn? A typed extension of the loading/streaming chunk protocol? Needs its own mini-manifest once phase 1 lands.
