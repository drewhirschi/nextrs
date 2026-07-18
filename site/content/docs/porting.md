+++
title = "Porting an Existing App"
description = "Start from the scaffold, graft your code into it, and convert route-by-route — the paved road for bringing an existing app to nextrs"
section = "Guides"
order = 6
+++

Two real production apps have been ported to nextrs — a [1.37M-LOC Next.js dashboard](/docs/case-study-port-at-scale) and a [~20k-LOC booking app](/docs/case-study-hhh). This page is the instructions those stories imply: what worked, in what order, and the contracts a port must respect. The case studies are the evidence; this is the procedure.

## Rule one: start from the scaffold, even for a port

The single biggest porting mistake is assembling nextrs **around** your existing code by hand — copying a `build.rs` from somewhere, hand-writing `main.rs`, improvising the client package. Every port that went smoothly did the opposite: it started from `create-nextrs-app` output and grafted the existing routes, auth, and database code **into** the generated skeleton.

The scaffold is not demo content — it is the wiring: `build.rs` codegen, the `client/` npm package and its orval pipeline, the `cargo dev` alias, the Vercel function entry, the prebuilt deploy script, and a `rust-toolchain.toml` pin that a hand-assembled app will not know it needs. Hand-rolling any of these means re-discovering, one confusing error at a time, decisions the scaffold already made.

Two ways to get the skeleton:

- **Fresh directory** (your existing app keeps living elsewhere — see the strangler pattern below):

  ```bash
  cargo install create-nextrs-app
  create-nextrs-app my-app-rs
  ```

- **Into an existing repo** — `--adopt` generates the same skeleton into a non-empty directory, minus the demo routes. It never overwrites: existing files are skipped and reported, an existing `src/main.rs` gets a `src/main.rs.example` beside it instead, and if you already have a `Cargo.toml` it prints the dependency lines to merge by hand:

  ```bash
  cd my-existing-repo
  create-nextrs-app --adopt --here
  ```

Then move your code in: your `route.ts` bodies become `route.rs` handlers, your auth becomes `middleware.rs`, your React pages drop into `app/**/page.tsx` — replacing the scaffold's example files rather than inventing parallel structure.

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
| Server-rendered page / server component | `app/**/page.tsx` + `app/**/prefetch.rs` (Rust pre-runs the data, seeds the React Query cache) |
| API route / route handler | `app/**/route.rs` — plain Axum handlers, `#[nextrs::api]` for the typed client |
| Server actions / RPC modules | `route.rs` endpoints + a same-signature TypeScript shim, so call sites don't change |
| Auth / route guards / `middleware.ts` | `middleware.rs` — scoped by directory placement, runs before anything renders |
| Layout | `layout.tsx` (React) or `layout.rs` + `layout.html` (Askama — remember `{{ children|safe }}`) |
| Loading / suspense skeleton | `loading.tsx` (or `loading.rs`/`.html`) — opts the route into streaming |
| DB layer | your Rust choice (both ports used `sqlx`) — called from `route.rs` and `prefetch.rs` |

## Contracts a port must respect

These are the conventions a hand-assembled port tends to miss. All of them are load-bearing.

### The `app/` tree is the router

Every directory under `app/` is a URL segment; the build step discovers the convention files and wires the router — there is no route registration to write. The slots: `page.{tsx,rs,html}`, `layout.tsx` **or** `layout.rs`+`layout.html`, `loading.{tsx,rs,html}`, `middleware.rs`, `api/**/route.rs`, and `prefetch.rs` (which requires a `page.tsx` sibling). A `.tsx` slot is exclusive — it cannot coexist with a `.rs`/`.html` of the same name. Full reference: [Routing Conventions](/docs/conventions).

### The generated `client/` package, and the bare-import rule

`client/` is a real npm package the scaffold generates: it holds the orval config, the generated hooks, and the seed helpers, and pages import it via the `@your-app/client` alias. Two consequences:

- **Every bare import your `.tsx` files use must be installed in `client/package.json`.** The embedded bundler resolves imports out of `client/node_modules`; since 0.3.6 it **errors** on unresolved bare imports instead of shipping a bundle that fails at runtime. When you copy components in from the old app, carry their dependencies into `client/package.json` and `npm install` — the bundle error names the missing specifiers.
- **Don't hand-write API types.** `route.rs` handlers annotated with `#[nextrs::api]` become an OpenAPI document, and `npm run gen` in `client/` regenerates typed React Query hooks from it. A Rust field rename breaks the TSX compile — that end-to-end check is most of the point of porting. See [Typesafe Client Generation](/docs/typesafe-client).

### The dev loop is `cargo dev`

The scaffold aliases `cargo dev` (in `.cargo/config.toml`) to the `cargo-nextrs-dev` watcher: it rebuilds and restarts on Rust, template, asset, and env changes, and the app wires live-reload in debug builds. Install it once with `cargo install cargo-nextrs-dev`. Don't substitute a hand-rolled watch script — the runner knows which inputs matter.

### Deploys are prebuilt

Scaffolded apps ship `scripts/deploy-prebuilt.sh` and a `vercel.json` with git auto-builds disabled: you compile locally (via `cargo-zigbuild`) and upload artifacts; deploys take seconds instead of a cloud cargo build plus queue. A port that copies only `vercel.json` and not the script — or vice versa — ends up with no working deploy path. Guide: [Deploy: Build Locally, Ship Artifacts](/docs/deploy-prebuilt).

## Gotchas

- **`SpeculationConfig` is about document speculation, not data seeding.** `SpeculationConfig` (named `PrefetchConfig` before 0.4.0; the old name still compiles with a deprecation warning) controls exactly one thing: the document-level `<script type="speculationrules">` tag that tells the *browser* which links to prefetch. It has nothing to do with the `/__nx/prefetch` data-seeding endpoint or with hover preloading — those are automatic. Since 0.4.0 speculation is **off by default**; opt in via `build_router_with_speculation` for server-rendered pages.
- **You never call `/__nx/prefetch` yourself.** Route chunk preloading and data prefetch on hover are automatic: the generated app shell preloads the target route's seeds through that endpoint on link intent. If you find yourself fetching `/__nx/prefetch` from app code, you're rebuilding a feature that's already on.
- **`prefetch.rs` needs a `page.tsx` sibling.** Next to a Rust page it's a compile error — Rust pages fetch their own data.
- **Askama layouts must use `{{ children|safe }}`** — without `|safe` the children are HTML-escaped and both your markup and the framework's streaming marker break.
- **Don't hand-edit generated output.** `client/src/generated/**`, `client/openapi.json`, and `public/dist/` are all regenerated by the build; changes there are silently lost. The seams for your code are `app/**`, `client/src/index.ts`, and `client/package.json`.

## When to bother

The [small-app case study](/docs/case-study-hhh) is blunt: at 20k LOC the JS dev loop is genuinely fast, and if your dev loop is your complaint, porting is not the fix. The reasons to port at any size are runtime — cold starts statistically indistinguishable from warm requests, ~2 orders of magnitude less memory, one small static binary — and, at scale, the dev loop too ([the 1.37M-LOC numbers](/docs/case-study-port-at-scale)). Read both before committing a team.
