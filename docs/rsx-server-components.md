# RSX Server Components: Rust Pages With React Islands

**Status:** v1 implemented (this PR, 2026-09-12) — `rsx!` macro, oxc props
extractor, island bundling + typed `crate::client` bindings, the
`pub async fn page(...)` convention, and the worked demo at
`examples/react-todos/app/server-stats/page.rs` +
`examples/react-todos/components/TodoStats.tsx`. Deviations from the original
sketch and open follow-ups are listed at the bottom.
**Motivation:** replace Askama templating for Rust-rendered pages (the `{{ }}` /
filter syntax has worn out its welcome) with a JSX-shaped `rsx!` macro, and let
those Rust server components embed real React `.tsx` components as typed,
hydrated islands.

## The idea in one paragraph

A `page.rs` is a **server component**: an async Rust function that does
server-only work (DB queries, auth) and returns HTML via an `rsx!` macro.
Inside that tree, imported `.tsx` React components render as **islands** —
placeholder elements carrying serialized props, mounted client-side by a tiny
runtime from per-component bundles. The novel piece is the **alien import**:
build-time codegen parses each island's TypeScript props and generates typed
Rust bindings, so `<TodoFilter counts={counts} />` in Rust type-checks against
the TSX component's props interface. Nobody in the Rust ecosystem has this —
Leptos/Dioxus/Yew all went all-Rust-via-WASM with zero React interop, and
nextrs today keeps whole pages in TSX. "Rust server components with React
client components as typed holes" is an empty niche.

## Target code

```tsx
// app/components/TodoFilter.tsx — plain React. No 'use client' directive:
// .tsx IS the client directive. The language boundary is the network boundary.
export interface TodoFilterProps {
  initialFilter: 'all' | 'active' | 'done';
  counts: { all: number; active: number; done: number };
}
export default function TodoFilter({ initialFilter, counts }: TodoFilterProps) {
  const [filter, setFilter] = useState(initialFilter);
  // ...
}
```

```rust
// app/todos/page.rs — no macro. Same convention as route.rs's `pub async fn get()`:
// the registry codegen discovers `page` and wires it as an Axum handler, so
// extractor-style params (db: Db) work exactly like they do in API routes.
use crate::client::TodoFilter;   // ← generated binding (the alien import)

pub async fn page(db: Db) -> Result<Rsx> {
    let todos = db.query_as::<Todo>("select * from todos order by created_at desc").await?;
    let counts = Counts::of(&todos);

    Ok(rsx! {
        <main class="p-8">
            <h1>"Todos"</h1>
            // Typed island: props checked at compile time against TodoFilterProps.
            <TodoFilter initial_filter="all" counts={counts} />
            <ul>
                { todos.iter().map(|t| rsx! { <li key={t.id}>{&t.title}</li> }) }
            </ul>
        </main>
    })
}
```

Renders as:

```html
<div data-nx-island="TodoFilter-a91f3c"
     data-nx-props='{"initialFilter":"all","counts":{"all":50,...}}'></div>
```

A ~1 KB client runtime scans for `data-nx-island`, looks up the chunk in the
build manifest, dynamic-imports it, and `createRoot`-mounts with the props.
No hydration reconciliation — server tree and client trees never overlap.

## Decisions made (and why)

1. **No `'use client'` directive.** Next.js needs it because one language
   shares one module graph. Here `.rs` physically cannot run in the browser
   and `.tsx` never runs on the server, so the file extension is the
   annotation. One less rule to teach.

2. **No `#[page]` attribute macro.** Convention over annotation, matching how
   `route.rs` already works (bare `pub async fn get()`, discovered by the
   registry codegen, extractors via Axum's Handler trait). A missing or misnamed
   `page` fn is a codegen-time build error. Metadata, if ever needed, becomes
   another convention (`pub const META` / `metadata()`), not an attribute.

3. **Alien imports are inferred from TSX, not declared in Rust** ("option A").
   TypeScript is the source of truth; the build generates
   `OUT_DIR/nextrs_client/mod.rs` with a props struct + component fn per
   island. This is the mirror image of the existing typed-client codegen
   (Rust routes → TS types), pointed the other way.

4. **`rsx!` is JSX-shaped** (angle brackets, expression holes `{expr}`),
   parsed with **`rstml`** — the parser crate under Leptos's `view!`, reusable
   directly. Inline `for`/`if` in markup (Dioxus precedent) is a taste option;
   iterator expression holes are the JSX-faithful baseline. Auto-escaping by
   default.

5. **Islands are leaves, not wrappers.** Server-rendered children flowing
   *through* client components (`<ClientTabs>{server_rsx}</ClientTabs>`) is
   the RSC flight-protocol rabbit hole — deliberately cut. Context providers
   live in the generated entry wrapper (where the React Query provider already
   lives today). Revisit only if real usage demands it.

6. **Props extraction: syntactic, with a strict allowlist.** See next section.

## The parser question

**Use oxc.** Rolldown is built on it, so `oxc_parser` / `oxc_ast` /
`oxc_semantic` are **already in the dependency tree** behind the `tsx` feature
(verified via `cargo tree`, pinned with rolldown 1.1.4). In-process, no Node,
no new dependency. Alternatives considered:

- **swc** — fine, but a second heavy dep doing what oxc already does here.
- **tsc (Node)** — full inference, but drags Node into the build loop.
- **tsgo (typescript-go)** — Microsoft's Go port of tsc, ~10x faster; the
  right *fallback tier* (shell out for `--emitDeclarationOnly`, parse the
  flattened `.d.ts`) if full type inference is ever needed. Not on the hot path.
- **ezno / stc** — experimental/abandoned Rust type-checkers; not viable.

Key caveat shaping the design: **oxc parses, it does not type-check.** So:

**House rule: island props must be explicitly annotated** with an
interface/type-literal of serializable shapes — primitives, string-literal
unions, arrays, optionals, nested object literals. Extraction is then a purely
syntactic walk: find the default export, take its first param's annotation,
resolve the named type (oxc_semantic symbol lookup), map members to Rust
types. Anything outside the allowlist — generics, conditional/mapped types,
`ReactNode`, types from node_modules — is a **build error naming the file,
prop, and offending type**. The error is a feature: island props must survive
JSON serialization anyway, so exotic types were never going to work; this
enforces the same wire-boundary discipline RSC imposes, at compile time.

**Cross-file types:** props interfaces may be imported from other project
files (`import type { Counts } from '../lib/types'`), so extraction must
follow project-local type imports across files (multi-file oxc pass). Types
from node_modules stay unsupported → build error.

**Casing seam (decide on day one):** `initialFilter` ↔ `initial_filter` — the
generated `Serialize` impl renames automatically so the JSON always matches
the TS side.

## Dependency graph

Two graphs, both already mostly handled:

- **Bundle graph:** an island root may import arbitrary other TSX/TS modules
  and npm packages. Rolldown already walks this — each island root becomes an
  entry point, shared deps land in shared chunks, same as the existing
  page-level bundling. Only the *roots* (components actually imported from
  RSX) need discovering; everything below them is ordinary module resolution.
- **Type graph:** the props extractor follows project-local `import type`
  edges as above.

## Build & dev loop (mostly exists)

1. `build.rs` (`bundle.rs`) already walks `app/`, emits the registry, runs
   Rolldown in-process. New steps in the same pass: extract island props →
   write `nextrs_client/mod.rs`; add per-island entry points; write
   `{island id → chunk URL}` manifest into the generated registry alongside
   the existing asset table.
2. `cargo:rerun-if-changed` + the `cargo-nextrs-dev` watcher already give the
   loop: save `TodoFilter.tsx` → bindings regenerate → Rust recompiles → the
   RSX import is live. One new artifact in an existing loop, not a new daemon.
3. **DX payoff:** bindings are generated Rust, so rust-analyzer autocompletes
   island props, and editing the TSX interface produces red squiggles in the
   `.rs` call site one rebuild later. Cross-language type errors in the
   editor is the demo moment.

## Landscape (why this and not an existing framework)

| | Syntax | Client story | JSX interop |
|---|---|---|---|
| Leptos | `view!` (JSX-like, rstml) | WASM, fine-grained signals, `#[island]` | none |
| Dioxus | `rsx!` (brace style) | WASM, VDOM full hydration | none |
| Yew / Sycamore | `html!` / `view!` | WASM | none |
| maud / hypertext | HTML macros, render-only | BYO JS | none |
| **this** | `rsx!` (JSX-like) | **real React islands** | **the point** |

The all-Rust frameworks trade away the React ecosystem (component libraries,
TanStack, Radix, hiring pool) for single-language purity. This design keeps
the ecosystem and gives Rust the server tree — the RSC split, with the
boundary enforced by the language instead of a directive.

## Implementation order (de-risk first)

1. **Spike: props extractor** — `fn extract_island_props(tsx: &str) ->
   Result<PropsShape, UnsupportedType>` on the vendored oxc crates, allowlist
   + good errors, then cross-file `import type` following. The only genuinely
   uncertain piece; everything else is plumbing that exists at coarser
   granularity.
2. `rsx!` macro on rstml: render + escape + expression holes + island calls.
3. Per-island Rolldown entries + manifest in the registry; `data-nx-island`
   placeholder protocol + mount runtime.
4. `page.rs` convention in discovery/registry codegen (mirror `route.rs`).
5. `examples/react-todos` grows an RSX page in the same PR that ships the
   feature (per CLAUDE.md: the demo app is the living reference), and `site/`
   starts migrating off Askama — it's the dogfood target for killing the
   old templating.

## v1 implementation notes (what shipped vs. the sketch)

- **Iteration holes:** a blanket `impl Render for I: Iterator` is impossible
  (coherence: conflicts with impls for foreign types like `&str`), so map
  chains end in `.collect::<Rsx>()` or wrap in `Rsx::each(...)`.
- **Island discovery:** `.rs` sources under `app/` and `src/` are scanned for
  `client::Name` references; each referenced name must match exactly one
  non-convention `.tsx` default export under `app/` or `components/`.
  Unreferenced `.tsx` files are never parsed for props, so existing colocated
  components can't break a build. Unknown names fall through to rustc's
  unresolved-import error.
- **No central mount runtime:** each island's Rolldown entry is
  self-mounting (querySelectorAll on its own `data-nx-island` id →
  `JSON.parse` props → `createRoot`), and `rsx_page` injects the matching
  `<script type="module">` tags (before `</body>` when present) by scanning
  the rendered HTML. The manifest lookup happens server-side at build time,
  not in the browser.
- **Optional props** generate `Option<T>` fields; struct-literal bindings
  mean the caller writes `title={Some(...)}` / `title={None}` explicitly.
  Builder-style bindings (omit optional props at the call site) are the
  natural follow-up.
- **TS `number` → `f64`**, so integer counts cross as `2.0`. Fine on the
  wire (JSON has one number type), mildly ugly in Rust literals.
- **Status flattening:** the registry's `PageFn` carries HTML only, so a
  non-200 from a `Result`-returning page is flattened into the body.
- **Islands-only apps** skip the TanStack app-shell entry entirely.
- **Scaffold** (`create-nextrs-app`) doesn't wire the `client` module or a
  starter island yet — follow-up alongside the docs-site page.

## Open questions

- Layouts: does `layout.rs` get RSX too, and how do RSX layouts compose with
  existing `layout.tsx`? (Likely: same slot mechanism, decide during phase 4.)
- Streaming/`loading.tsx` interaction: RSX pages render fully before send in
  v1; streaming SSR is a separate track (see docs/streaming.md).
- Whether `rsx!` allows inline `for`/`if` (Dioxus-style) in addition to
  expression holes, or holes only.
- Coexistence/migration story for existing Askama pages — presumably both
  conventions run side by side indefinitely, Askama soft-deprecated in docs.
