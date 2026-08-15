+++
title = "Porting an Existing App"
description = "Start from the scaffold, graft your code into it, and convert route-by-route — the paved road for bringing an existing app to nextrs"
section = "Guides"
order = 6
+++

Two real production apps have been ported to nextrs — a [1.37M-LOC Next.js dashboard](/docs/case-study-port-at-scale) and a [~20k-LOC booking app](/docs/case-study-hhh). This page is the instructions those stories imply: what worked, in what order, and the contracts a port must respect. The case studies are the evidence; this is the procedure.

## Rule one: start from the scaffold, even for a port

The single biggest porting mistake is assembling nextrs **around** your existing code by hand — copying a `build.rs` from somewhere, hand-writing process entry points, or improvising the client package. Every port that went smoothly did the opposite: it started from `nextrs new` output and grafted the existing routes, auth, and database code **into** the generated skeleton.

The scaffold is not demo content — it is the wiring: `build.rs` codegen, the hidden linked client and its Orval/TypeScript pipeline, the `cargo dev` alias, the shared `src/app.rs`, the Vercel process adapter, the prebuilt deploy script, and a `rust-toolchain.toml` pin. Hand-rolling these means re-discovering, one confusing error at a time, decisions the scaffold already made.

Two ways to get the skeleton:

- **Fresh directory** (your existing app keeps living elsewhere — see the strangler pattern below):

  ```bash
  cargo install cargo-nextrs
  nextrs new my-app-rs
  # equivalent: cargo nextrs new my-app-rs
  ```

- **Into an existing repo** — `--adopt` generates the same skeleton into a non-empty directory, minus the demo routes. It never overwrites: existing files are skipped and reported, an existing `src/main.rs` gets a `src/main.rs.example` beside it instead, and if you already have a `Cargo.toml` it prints the dependency lines to merge by hand:

  ```bash
  cd my-existing-repo
  nextrs new --adopt --here
  ```

Then move your code in: your `route.ts` bodies become `route.rs` handlers, your auth becomes `middleware.rs`, your React pages drop into `app/**/page.tsx`, shared React UI can move into `components/`, and Rust domain code belongs in `src/` — replacing the scaffold's example files rather than inventing parallel structure.

## The strangler pattern: convert route-by-route

Neither case-study port was a big-bang rewrite of a live system. The existing app keeps serving; nextrs takes over route-by-route. The shape that worked:

1. **Inventory first.** Walk the existing route tree and write a worksheet (`MIGRATION.md`) with one row per route: URL, data dependencies, auth requirements, and the nextrs target files. If the app uses server actions or RPC, add a second table — one row per module and function. In action-heavy apps *that* table, not the route list, is the real API surface.
2. **Keep the frontend identical.** Client-rendered React components port nearly unchanged into `app/**/page.tsx` — the 1.37M-LOC port reused its ~768k-LOC React UI byte-for-byte. What gets rewritten is everything behind the components: the Node server becomes one Rust binary.
3. **Convert leaf routes first**, one vertical slice at a time: `page.tsx` + its `route.rs` endpoints + `prefetch.rs` seed + `middleware.rs` guard. Verify the slice end-to-end (same wire shapes, same flows) before the next.
4. **Bridge what you can't port yet.** The booking-app port ran its auth as a sidecar first, then ported it natively and oracle-diffed 48/48 responses against the live sidecar before deleting it. A temporary proxy from the nextrs app to the old backend (or routing at your edge/CDN, path-by-path) keeps both halves live during the transition.
5. **Diff against the original as you go.** Byte-level wire parity on representative endpoints is cheap to check and catches semantic drift early. Porting is an audit — the booking-app conversion found three latent bugs in the original.

Where each old concept lands:

| You have | nextrs target |
|---|---|
| Client-rendered React page | `app/**/page.tsx` (unchanged, client-rendered) |
| Next.js server component | `app/**/page.tsx` + `app/**/prefetch.rs` (Rust pre-runs the data, seeds the React Query cache) |
| API route / route handler | `app/**/route.rs` — plain Axum handlers, `#[nextrs::api]` for the typed client |
| Server actions / RPC modules | `route.rs` endpoints + a same-signature TypeScript shim, so call sites don't change |
| Auth / route guards / `middleware.ts` | `middleware.rs` — scoped by directory placement, runs before anything renders |
| Layout | `layout.tsx` |
| Loading / suspense skeleton | `loading.tsx` |
| Route-local React component | Any ordinary filename beside its page, such as `TodoRow.tsx` |
| Shared React component | Top-level `components/` |
| DB layer | your Rust choice (both ports used `sqlx`) — called from `route.rs` and `prefetch.rs` |

## Contracts a port must respect

These are the conventions a hand-assembled port tends to miss. All of them are load-bearing.

### The `app/` tree is the router

Every directory under `app/` is a URL segment; the build step discovers exact
convention filenames and wires the router. `page.tsx`, `layout.tsx`,
`loading.tsx`, and `not-found.tsx` are React slots. Rust supplies
`middleware.rs`, `route.rs`, and an optional `prefetch.rs` beside a React page.
Other files are ordinary colocated modules and do not create routes. Full reference:
[Routing Conventions](/docs/conventions).

### The generated client is a real hidden package

`.nextrs/client` is a genuine npm workspace package generated by the framework,
not a place for application components. The root `package.json` links it into
`node_modules` and owns every JavaScript dependency. Fetch functions come from
`@your-app/client`; hooks and query/mutation helpers come from
`@your-app/client/react-query`.

- **Every bare import used by `.tsx` code belongs in the root `package.json`.** Run `npm install` only at the app root. Never install dependencies inside `.nextrs/client`.
- **Don't hand-write API types.** `route.rs` handlers annotated with `#[nextrs::api]` become an OpenAPI document, and `cargo nextrs client generate` at the app root regenerates typed fetch functions and React Query hooks. A Rust field rename breaks the TSX compile — that end-to-end check is most of the point of porting. See [Typesafe Client Generation](/docs/typesafe-client).
- **Don't add resolution shims.** The generated package emits JavaScript and `.d.ts` for both entry points. New nested files resolve them through normal package exports, without `tsconfig.paths`, relative generated imports, or `declare module` files.

### The dev loop is `cargo dev`

The scaffold aliases `cargo dev` (in `.cargo/config.toml`) to the watcher bundled with `cargo-nextrs`: it rebuilds and restarts on Rust, template, asset, and env changes, and the app wires live-reload in debug builds. Install the one CLI with `cargo install cargo-nextrs`. Don't substitute a hand-rolled watch script — the runner knows which inputs matter.

`cargo nextrs dev` and `nextrs dev` are the direct equivalents. The unified dev
command refreshes the generated client before watching.

### The Rust app is shared; process entry points are thin

`src/app.rs` constructs the Axum `Router` and owns application-wide layers.
`src/main.rs` starts the local/container process. `api/index.rs` only adapts
the same app to Vercel's required Rust function entry. `build.rs` remains the
normal Rust build script for route/OpenAPI discovery and TSX bundling.

If Vercel is not a target, remove `api/index.rs`, the `index` Cargo target,
Vercel-only dependencies, `vercel.json`, and the prebuilt deploy script as one
unit. Do not delete the shared `src/app.rs` or local `src/main.rs`.

### Deploys are prebuilt

Scaffolded apps ship `scripts/deploy-prebuilt.sh` and a `vercel.json` with git auto-builds disabled: you compile locally (via `cargo-zigbuild`) and upload artifacts; deploys take seconds instead of a cloud cargo build plus queue. The same `vercel.json` also contains a self-sufficient root `npm ci` and client/Cargo build for anyone who deliberately re-enables cloud builds. Guide: [Deploy: Build Locally, Ship Artifacts](/docs/deploy-prebuilt).

## Gotchas

- **You never call `/__nx/prefetch` yourself.** Route chunk preloading and data prefetch on hover are automatic: the generated app shell preloads the target route's seeds through that endpoint on link intent. If you find yourself fetching `/__nx/prefetch` from app code, you're rebuilding a feature that's already on.
- **`prefetch.rs` needs a `page.tsx` sibling.** It exists to warm that React page's query cache.
- **Don't hand-edit generated output.** `.nextrs/openapi.json`, the complete `.nextrs/client/**` package, and `public/dist/` are regenerated and ignored. The tracked `.nextrs/template/client` wiring recreates the workspace target automatically. Application seams are `app/**`, `components/**`, `src/**`, and the root `package.json`.

## When to bother

The [small-app case study](/docs/case-study-hhh) is blunt: at 20k LOC the JS dev loop is genuinely fast, and if your dev loop is your complaint, porting is not the fix. The reasons to port at any size are runtime — cold starts statistically indistinguishable from warm requests, ~2 orders of magnitude less memory, one small static binary — and, at scale, the dev loop too ([the 1.37M-LOC numbers](/docs/case-study-port-at-scale)). Read both before committing a team.
