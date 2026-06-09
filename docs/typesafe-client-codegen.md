# Generating a typesafe TS/JS client for every `route.rs`

> **Status: Option A implemented.** We went with the OpenAPI-first approach
> (`utoipa` + `orval`) specifically to get generated React Query hooks. See the
> "Decision" section below and the `site/client/` directory for the working
> pipeline. The rest of this doc is the original options analysis, kept for
> context.

**Target: a TypeScript/JavaScript client.** That crosses the Rust→JS language
boundary, so the question is which mechanism carries Rust types across it. A
shared-Rust-types approach (compiler-enforced, no schema) is off the table here —
it only helps a Rust consumer. The realistic choices are an OpenAPI spec
(language-agnostic) or direct Rust→TS type export.

## What we have today

`nextrs` discovers API endpoints by convention: any `app/**/route.rs` that
exports `pub async fn {get,post,put,patch,delete,head,options}` becomes a method
handler at the directory's URL path. The build-time codegen
(`nextrs/src/build.rs`) text-scans each `route.rs` for those function names and
emits a `generated_registry()` that wires them into an Axum router.

The handler contract today is **untyped at the edges**:

```rust
// site/app/api/ping/route.rs
pub async fn get(_req: Request<Body>) -> impl IntoResponse { StatusCode::OK }
pub async fn post(_req: Request<Body>) -> impl IntoResponse { StatusCode::CREATED }
```

`route_method()` (`nextrs/src/conventions.rs`) accepts any
`Fn(Request<Body>) -> impl IntoResponse`. The framework never sees a request or
response *type* — it sees `Body` in and `Response` out. The frontend is
server-rendered Askama HTML with no JS framework and no API client at all.

**This is the central problem.** You cannot generate a typesafe client for
`Body -> impl IntoResponse`. There is no contract to project. So every option
below has the same prerequisite, and the options differ mainly in *how* the
contract is declared and *what* the client is written in.

---

## The prerequisite: a typed handler contract

A typesafe client needs, per (path, method): the request type (body / query /
path params) and the response type. We have to make handlers *declare* those.
Two broad shapes:

### Shape A — Axum extractors (idiomatic, structural)

```rust
pub async fn post(Json(body): Json<CreateUser>) -> Json<User> { ... }
pub async fn get(Query(q): Query<ListParams>) -> Json<Vec<User>> { ... }
```

The signature itself carries the contract. Pro: zero new concepts, Axum already
does the runtime (de)serialization. Con: codegen must *parse Rust types out of
function signatures*, and `impl IntoResponse` erases the response type — you'd
have to require a concrete return type (`Json<T>`, not `impl IntoResponse`).

### Shape B — explicit associated types / convention aliases

```rust
// route.rs declares the contract by name, codegen keys off the alias
pub type Post = (CreateUser, User);   // (Request, Response)
pub async fn post(Json(b): Json<CreateUser>) -> Json<User> { ... }
```

Or a trait:

```rust
impl Endpoint for Post { type Req = CreateUser; type Res = User; }
```

Pro: explicit, easy for codegen to find. Con: redundant with the signature, easy
to let them drift unless a macro ties them together.

**Either way**, request/response structs must derive `Serialize`/`Deserialize`
(serde is already a dependency) — and, for cross-language clients, something
that can emit a schema or a TS type.

---

## The options

Two things have to make it to TS: the **types** (request body, query, path
params, response) and the **call table** (which URL + method each typed function
hits). For the call table, nextrs is already 90% there — discovery enumerates
every (path, method) at build time. The options differ mainly in how the
**types** cross the boundary, plus how much they fight the file-convention
routing.

### Option A — OpenAPI-first via `utoipa` or `aide`

Make the app emit an OpenAPI 3 document, then generate the TS client from it with
`openapi-typescript` (types only, pair with a thin fetch wrapper) or
`openapi-generator` / `orval` / `openapi-fetch` (full typed client, optionally
with React Query hooks).

- **Type safety:** strong, but mediated by a spec — a loose schema
  (`additionalProperties`, untyped `serde_json::Value`) silently degrades to
  `any`/`unknown`.
- **What we own:** the annotation/collection step. Client gen is off-the-shelf
  and battle-tested.
- **Fit with nextrs:** the weak point. `utoipa` wants a `#[utoipa::path(...)]`
  attribute on every handler that restates the method/path/body/response —
  duplicating exactly what the file convention already encodes, and easy to let
  drift. `aide` infers more from Axum extractors but expects you to register
  routes programmatically with its `ApiRouter`; nextrs builds the router from the
  generated `RouteRegistry`, so you'd thread schema collection through
  `RouteEntry` and the codegen. Neither is a clean drop-in.
- **Cost:** heavy deps; a spec artifact to keep in CI.
- **Upside:** Swagger UI and a documented, polyglot, public-facing API for free;
  any future non-TS consumer is already covered; generators support React Query
  / SWR hooks out of the box.

Best if the API is (or will become) public/polyglot, or you want interactive docs
and don't mind the ceremony.

### Option B — Direct Rust→TS type export (`ts-rs`, `typeshare`, or `specta`)

Skip OpenAPI. Derive TS types straight from the Rust request/response structs,
then emit a typed `fetch` client keyed off nextrs's existing endpoint
enumeration.

- `ts-rs`: `#[derive(TS)]` on each struct; `.ts` files are written during
  `cargo test`. Simplest, most popular, no external CLI.
- `typeshare`: annotations + a standalone CLI; also targets Swift/Kotlin if you
  ever want native clients.
- `specta`: richest type support; pairs with `rspc` for a tRPC-style typed RPC
  layer (a bigger architectural commitment than plain REST handlers).

- **Type safety:** good end-to-end TS *types*. The *call* (URL/method/params)
  comes from our build-time table, so safety of the call is only as good as the
  glue codegen we write to bind types to endpoints.
- **What we own:** the endpoint→type binding and the fetch-wrapper emitter — a
  natural extension of `build.rs`. Type export itself is the library's job.
- **Fit with nextrs:** good and lightweight. No router rewrite; add derives to
  the structs and extend the existing codegen. It matches the framework's
  "convention + build.rs" philosophy.
- **Cost:** light, no spec artifact. Downside: bespoke — no ecosystem of
  ready-made consumers/hooks like OpenAPI has; you build the fetch layer.

Best for exactly this case: a TS browser client, thin pipeline, minimal ceremony.

### Option C — Fully bespoke `syn` codegen

Teach `build.rs` to parse each `route.rs` with `syn` and emit both the registry
*and* the TS types + client directly.

- **Type safety:** as good as the parser you write.
- **Fit:** philosophically native (the framework *is* convention + codegen) but
  by far the most code to own. The current discovery deliberately avoids a Rust
  parser ("without pulling a Rust parser into the build feature" —
  `build.rs:250`), and reimplementing serde's type→JSON/TS mapping by hand is a
  tar pit. Don't hand-roll what `ts-rs`/`utoipa` already do.

Only if zero external client-gen deps is a hard requirement.

---

## Comparison (TS/JS client)

| | Type safety | What we own | nextrs fit | Deps/weight | Hooks/ecosystem |
|---|---|---|---|---|---|
| A. OpenAPI (`utoipa`/`aide`) | Strong (via spec) | Annotations / collection | Moderate — fights file routing | Heavy | React Query/SWR, Swagger UI |
| B. Rust→TS (`ts-rs`/`specta`) | Good (types); call via our table | Endpoint→type bind + fetch wrapper | Good — extends existing codegen | Light | DIY (or specta+rspc) |
| C. Bespoke `syn` | As good as parser | Everything | Native but heaviest | Minimal ext. | DIY |

---

## Recommendation

For a TS/JS-only consumer, start with **Option B (`ts-rs`)**. It's the cleanest
fit: nextrs already enumerates every endpoint at build time, so we extend that
same codegen to emit a typed `fetch` client and let `ts-rs` handle the
struct→TS projection. Light deps, no spec to babysit, and it stays within the
framework's convention-and-codegen model.

Reach for **Option A (`aide`)** instead if the API will be public/polyglot or you
want generated React-Query hooks and Swagger docs — accept that you'll have to
thread schema collection through `RouteRegistry`, since the per-handler
`utoipa` attributes duplicate the file convention. Avoid **Option C** unless
external deps are forbidden.

### Prerequisites (both A and B)

1. **Adopt a typed handler contract.** Today handlers are
   `Request<Body> -> impl IntoResponse`, which exposes no types. Switch to Axum
   extractors with a *concrete* return type so the response is recoverable:
   ```rust
   pub async fn post(Json(b): Json<CreateUser>) -> Json<User> { ... }
   ```
   `impl IntoResponse` must go for any endpoint we want to type. Add the
   request/response derives the chosen tool needs (`#[derive(TS)]` for Option B;
   `ToSchema` for `utoipa`).
2. **Type the path params.** `[id]` already becomes `{id}` in discovery
   (`discovery.rs` / `build.rs`); the generated TS function should take those as
   typed arguments and interpolate the URL.
3. **Guard untypable handlers.** The codegen already emits `compile_error!` for
   page/route GET conflicts (`build.rs:273`); do the same when a `route.rs`
   method we're meant to type still uses raw `Body`/`impl IntoResponse`, so the
   contract can't silently rot to `any`.

### Suggested first slice

Prove it on the one existing endpoint, `/api/ping`:

1. Give it typed request/response structs deriving `Serialize`/`Deserialize` +
   `TS`, and switch the handlers to `Json<T>` signatures.
2. Have `cargo test` (ts-rs) emit the `.ts` types into a `site/client/` dir.
3. Extend `build.rs` to emit a `client.ts` with one typed function per
   (path, method) — `apiPing.get()`, `apiPing.post(body)` — alongside
   `generated_registry()`.

That validates the type-export + endpoint-enumeration pipeline end to end with
minimal commitment; the same enumeration can later back an OpenAPI emitter if a
polyglot consumer shows up.

---

## Decision (implemented)

We chose **Option A (OpenAPI-first)**. The deciding factor was wanting
**generated React Query hooks** — `orval` produces them directly from an
OpenAPI document, so the whole client (hooks + types) is generated, not just
the types.

### How it fits nextrs

The concern with Option A was that `utoipa` "fights the file-convention
routing." We resolved that by letting nextrs's **existing build-time route
discovery do the collection** instead of a hand-maintained `#[derive(OpenApi)]`
list:

- The codegen (`nextrs/src/build.rs`) already enumerates every `route.rs`
  method. It now also detects which methods carry a `#[utoipa::path]`
  annotation and emits a `generated_openapi()` function listing exactly those.
  Annotation is **opt-in per handler** — an un-annotated handler still routes,
  it just doesn't appear in the spec/client.
- `route_method` (`nextrs/src/conventions.rs`) was widened from
  `Fn(Request<Body>) -> impl IntoResponse` to **any Axum `Handler`**, so
  handlers can use typed extractors (`Json<T>`, `Query<T>`, …) with concrete
  return types. Old raw-request handlers still compile unchanged.
- The codegen **verifies each annotation's `path = "..."` against the
  file-convention URL** and emits a `compile_error!` on mismatch (same spirit as
  the existing page/route GET-conflict guard). So the path stays hand-written —
  utoipa requires it — but it can't silently drift from the file's location.
- `nextrs::openapi::spec_router` serves the document at `/openapi.json`; both
  the dev server and the Vercel entrypoint merge it in.

### What the annotation needs

With the `axum_extras` feature on, a handler's `#[utoipa::path]` needs only:

```rust
#[utoipa::path(post, path = "/api/ping", responses((status = 200, body = PingResponse)))]
pub async fn post(Json(req): Json<PingRequest>) -> Json<PingResponse> { ... }
```

- `method` + `path` — required by utoipa (`path` is checked against the file).
- `responses(...)` — required for a *typed response*; utoipa does not infer it
  from the `Json<T>` return type.
- the **request body is inferred** from the `Json<T>` extractor — no
  `request_body = ...`.
- `operation_id` / `tag` — optional; they just give the generated hook a clean
  name (`useSendPing`) and group it.

### The pipeline

```
route.rs (#[utoipa::path]) ─build.rs codegen→ generated_openapi()
  → served at /openapi.json   and   dumped to site/client/openapi.json
  → orval → site/client/src/generated/** (React Query hooks + TS types)
```

`cargo run -p site --bin dump-openapi` writes the spec to disk; `orval` turns
it into hooks. `cd site/client && npm run gen` runs both. See
`site/client/README.md`.

### Known limitations / follow-ups

- `operation_id` / `tag` are set per handler for clean hook names; we set them in
  the `/api/ping` example and document the convention rather than inferring them.
  (Deriving them from the route in codegen is possible but was deliberately left
  out — explicit names keep the generated hooks predictable.)
- The `path` still has to be written by hand because utoipa's macro requires it;
  the codegen guards it against drift but doesn't supply it. Fully deriving it
  would need a nextrs attribute macro (a proc-macro *can* read its source file on
  current stable via `Span::file()`), which is a larger change than the guard.
- Swagger UI isn't bundled (it would pull a heavy build-time asset dependency);
  the raw `/openapi.json` is enough to drive client generation. Adding
  `utoipa-redoc` or `utoipa-swagger-ui` behind a feature is a possible
  follow-up.
