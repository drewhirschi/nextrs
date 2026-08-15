+++
title = "API Routes"
description = "Handler return shapes: bodies, status codes, headers, and typed errors"
section = "Guides"
order = 4
+++

A `route.rs` under `app/` exports one async function per HTTP method (`get`,
`post`, `patch`, …), annotated with `#[nextrs::api]`. The macro derives the
OpenAPI path from the file location and infers the operation's request and
response types from the signature — so the signature is the contract, and the
generated TypeScript client can't drift from it.

This page is about the *return* side of that contract: how you produce a body,
a status code, headers, and errors — and what each choice costs you in
generated-client fidelity.

## The three tiers

| Return type | Status | Codegen |
|---|---|---|
| `Json<T>` | always 200 | full: typed client + seed companion |
| `Result<Json<T>, ApiError>` | 200 or the error's status | full, **including a typed error body** |
| anything else `IntoResponse` | yours to build | none inferred — declare `responses(...)` by hand |

The framework never forces a shape — every handler is an ordinary Axum handler
— but the further down the table you go, the more you have to state manually.

## Bodies

Return `Json<T>` where `T: Serialize + ToSchema`. The macro reads `T` out of
the return type and registers it as the 200 response, which is what types the
generated hook (`useGetApiTodosById` returns `TodoDetail`, not `unknown`).

Don't return a bare `T` — there is deliberately no "you probably meant JSON"
inference. `Json(value)` is one wrapper and makes the wire format explicit.

One gotcha: **type aliases defeat inference**. `-> ApiResult<Json<T>>` looks
tidy but a proc-macro can't resolve aliases, so the operation silently loses
its response schema *and* its seed companion. Spell the return type out.

## Errors: `Result<Json<T>, ApiError>`

The recommended shape for anything fallible. `nextrs::ApiError` carries a
status code plus a typed JSON body:

```rust
use nextrs::ApiError;

#[nextrs::api(get)]
pub async fn get(Path(id): Path<u64>) -> Result<Json<TodoDetail>, ApiError> {
    let todo = ctx.get(id).await
        .ok_or_else(|| ApiError::not_found("no todo with that id")
            .with_code("todo_not_found"))?;
    Ok(Json(todo.into()))
}
```

On the wire an error is the status plus `{"error": "...", "code": "..."}`
(`code` optional, for clients that branch on failures without string-matching).
Constructors exist for the common statuses — `bad_request`, `unauthorized`,
`forbidden`, `not_found`, `conflict`, `unprocessable`, `internal` — and
`ApiError::new(status, msg)` covers the rest. `From<StatusCode>` is implemented,
so handlers that used `?` on a `StatusCode` migrate by changing the return type.

Because the macro recognizes the shape structurally, a `Result<Json<T>,
ApiError>` handler **self-registers a `default` error response with the
`ApiError` schema** — no `responses(...)` block. The generated client then has
a typed error union, not just a typed success.

Your own error enum works too: implement `IntoResponse` (most apps convert to
`ApiError` internally) and declare its responses on the attribute. If you do
declare `responses(...)`, you only need the *error* entries — the inferred 200
is merged in whenever your block doesn't declare a success status:

```rust
#[nextrs::api(responses((status = 404, description = "unknown org", body = ApiError)))]
pub async fn get(...) -> Result<Json<OrgDetail>, ApiError> { ... }
// spec gets: 200 → OrgDetail (inferred), 404 → ApiError (declared)
```

## Status codes

- An infallible `Json<T>` is a 200; a `Result` is 200 or the error's status.
  That covers most routes.
- A non-200 success (a `201`, say) is Axum tuple composition:
  `(StatusCode::CREATED, Json(created))`. Tuple returns aren't inferred yet,
  so declare the response: `responses((status = 201, body = Todo))`.
- A body-less handler can return `StatusCode` directly (the react-todos
  `delete` does) — fine for endpoints the typed client only calls for effect.
  A bare `StatusCode` return infers a body-less 200, so the operation still
  appears in the spec without a `responses(...)` block.

## Headers

Also Axum composition — anything `IntoResponseParts` stacks in front of the
body:

```rust
use axum::http::header::SET_COOKIE;
use axum::response::AppendHeaders;

pub async fn post(...) -> (AppendHeaders<[(HeaderName, String); 1]>, Json<Session>) {
    (AppendHeaders([(SET_COOKIE, cookie)]), Json(session))
}
```

`HeaderMap` works the same way, and both compose with a status:
`(StatusCode, headers, Json<T>)`. As with non-200 successes, tuples currently
need a hand-written `responses(...)` for the body to reach the spec.

For response timing there's a shortcut: take a `nextrs::Timing` extractor and
wrap work in `timing.span("db", fut)` — the `Server-Timing` header is emitted
for you (see [Route Telemetry](/docs/telemetry)).

## The escape hatch

Any `impl IntoResponse` — streams, redirects, raw `Response` — is a valid
handler. The macro can't see through it, so infer nothing: declare
`responses(...)` yourself if the operation should appear typed in the client,
and know that opaque GETs never get a seed companion. That's the trade: full
Axum freedom, manual contract.

## Where the pieces live

Routing (which files become routes, dynamic `[id]` segments, method naming) is
covered in [Routing Conventions](/docs/conventions); how the spec becomes a
typed client is [Client Generation](/docs/client-codegen). The worked example
for everything above is `examples/react-todos/app/api/todos/` in the repo.
