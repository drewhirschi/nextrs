# `ApiError` — a typed error convention for API routes

- **Reported-in:** design discussion (2026-08-14), building on linkedin-challenge's
  responses-inference report and onenote-extractor's fallible-seeding report
- **Date:** 2026-08-14
- **Status:** fixed in 0bad81e

## Problem

The framework had a success-side convention (`Json<T>`, inferred into OpenAPI
and the generated client) but no error-side one. Real handlers returned
`Result<Json<T>, StatusCode>` — an opaque status with no body — and any error
that *should* appear in the spec had to be hand-written in `responses(...)`,
which (per [[api-macro-responses-inference]]) also forced restating the 200.
The generated client's error side was untyped, which matters double for the
planned plain typed fetch client where errors-as-a-typed-union is the payoff.

## Direction (agreed 2026-08-14)

A tiered return-shape convention — never forced, but each tier states its cost:

1. `Json<T>` — infallible simple case stays simple. Full codegen + seed.
2. `Result<Json<T>, ApiError>` — **recommended for anything real.**
   `nextrs::ApiError` carries `(status, typed JSON body {error, code?})`,
   implements `IntoResponse` + `ToSchema` + `From<StatusCode>`, and the macro
   recognizes it structurally (last path segment `ApiError`) to inject a
   `default` error response with its schema — self-registering, no
   `responses(...)` block.
3. Anything `IntoResponse` — escape hatch; nothing inferred, hand-declare
   `responses(...)` for client typing, no seed companion.

Hard line: no bare `T` returns — `Json(x)` stays explicit.

Supporting macro change: merge-not-replace inference — inject the inferred
`(status = 200, body = T)` into a user-written `responses(...)` whenever it
declares no success status (fixes problem 1 of
[[api-macro-responses-inference]]).

## Implementation Notes

- `crates/nextrs/src/error.rs` — `ApiError` with status shorthands
  (`not_found`, `bad_request`, …) and `.with_code(...)`.
- `crates/nextrs-macros/src/lib.rs` — `infer_api_error` (Result's second
  generic arg, last segment `ApiError`, emitted as written so path qualifiers
  survive), `declares_success_response`, `merge_success_into_responses`.
- Demo: react-todos detail GET returns `Result<Json<TodoDetail>, ApiError>`
  with **no** responses block; spec + generated TS client verified to carry
  the typed `ApiError`, live 404 curl'd.
- Docs: new guide `site/content/docs/api-routes.md` (bodies / status codes /
  headers / errors / escape hatch), Guides order 4.

## Validation

- `nextrs::error` unit tests: body shape, code, response status+content-type,
  `From<StatusCode>`.
- Macro tests: `infer_api_error` recognition (incl. `nextrs::ApiError`
  qualifier, StatusCode rejected), success-declaration detection, merge
  splicing.
- Demo end-to-end: regenerated `.nextrs/openapi.json` has `200` + `default:
  ApiError`; generated client exports `ApiError` interface; live server 404
  returns JSON error body.
