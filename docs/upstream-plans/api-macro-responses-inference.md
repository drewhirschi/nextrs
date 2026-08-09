# `responses(...)` is all-or-nothing, and type aliases defeat inference

- **Reported-in:** linkedin-challenge (17 annotated routes)
- **Date:** 2026-08-09
- **Status:** reported

## Problem 1: declaring an error response forces you to restate the success one

The macro injects the inferred success response only when the attribute contains
no `responses(` at all:

```rust
if !extra_args.contains("responses(") && !extra_args.contains("responses (") {
    if let Some(body) = func.as_ref().and_then(infer_success_body) { … }
}
```

So a route that wants to document a 404 must also hand-write the 200 that the
return type already states. In practice most real routes are fallible, so the
verbose form survives almost everywhere — the inference mainly helps the routes
that needed it least. Of 17 routes in this app, only 6 could drop the block.

```rust
// What the return type already says, restated because of one 404:
#[nextrs::api(
    operation_id = "getOrg",
    responses(
        (status = 200, description = "The org and its competitions", body = OrgDetail),
        (status = 404, description = "No such organization", body = ApiError),
    ),
)]
pub async fn get(…) -> Result<Json<OrgDetail>, ApiError> { … }
```

**Suggestion:** merge rather than replace. Inject the inferred `(status = 200,
body = T)` whenever the user's `responses(...)` does not already declare a 200.
That is a small change to the same condition and makes the common shape:

```rust
#[nextrs::api(responses((status = 404, body = ApiError)))]
pub async fn get(…) -> Result<Json<OrgDetail>, ApiError> { … }
```

The eventual goal — deriving error responses from a trait on the error type —
subsumes this, but the merge is worth having on its own and is not blocked by it.

## Problem 2: a type alias silently disables inference

`infer_success_body` matches on the last path segment of the return type, so it
sees `Json` or `Result` but not an alias for either. A codebase with the very
ordinary

```rust
pub type ApiResult<T> = Result<T, ApiError>;
```

gets **no** inferred response from `-> ApiResult<Json<Foo>>`, and — because the
attribute then contains no `responses(...)` — silently emits an operation with
an empty `responses: {}`. The generated client's success type degrades with no
warning. We hit exactly this: two operations lost their 200 during a cleanup and
only a before/after diff of `openapi.json` caught it.

`nextrs::build::emit_seeds` has the same blind spot for the same reason
(`ret_is_seedable` is a normalized prefix match), so an aliased handler also
silently loses its seed companion — see `fallible-handler-seeding.md`.

Aliases cannot be resolved in a proc macro, so this may be unfixable directly.
Two cheaper mitigations:

- **Warn instead of failing silently.** If the return type is neither `Json<…>`
  nor `Result<Json<…>, _>` nor `()`/`StatusCode`, and the user wrote no
  `responses(...)`, emit a compile warning naming the type. An empty `responses`
  block is nearly always a mistake, and it is invisible today.
- **Document it** in the client-generation guide: spell the return type out,
  because `ApiResult<T>` costs you both the response schema and the seed
  companion. This is non-obvious — the alias is exactly the sort of tidying a
  Rust developer does by reflex.

## Also: tuple returns

`(HeaderMap, Json<T>)` — the idiomatic way to set a cookie alongside a body —
infers nothing, so any route that sets a header must declare its response by
hand. Reaching one level into a tuple for the first `Json<…>` would cover the
auth routes in this app (login, signup, join, logout), which are exactly the
places a `Set-Cookie` is needed.
