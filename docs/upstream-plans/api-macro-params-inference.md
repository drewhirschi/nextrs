# `#[nextrs::api]` Should Infer `params(...)` from the Handler Signature

- **Reported-in:** onenote-extractor
- **Date:** 2026-07-01
- **Status:** fixed in 30ec207

## Problem

Handlers declare params once as axum extractors (`Path<(i64, i64)>`,
`Query<T>`) and again in the macro's utoipa `params(...)` list. Forgetting the
macro side silently drops the params from the OpenAPI spec — and therefore from
the generated TypeScript client's types — with no build error. The two
declarations can also drift in type.

## Proposed Direction

The extractors become the single source of truth. When the user does not write
`params(...)`, the macro derives it:

- `Path<T>` args zip with the `{seg}` names the macro already extracts from the
  file-derived URL: scalar `T` ↔ one segment, tuple `(A, B)` ↔ segments in
  order, a single named struct across multiple segments → `params(T)`
  (requires `IntoParams`).
- `Query<T>` contributes `params(T)` (requires `IntoParams`).
- A user-written `params(...)` always wins — nothing is injected.

## Implementation Notes

- The macro already computes the URL via `url_from_file`; `{seg}` names come
  from there. Parse the fn item with `syn` (already a dependency for the seed
  companion).
- Mixed lists are valid utoipa syntax:
  `params(("id" = i64, Path), FilterQuery)`.
- Catch-all `{*seg}` params map to `String`.
- If the segment count and `Path` type shape can't be reconciled, inject
  nothing (utoipa/compile errors stay the user's signal, as today).

## Validation

- Unit tests on the inference: scalar Path, tuple Path, struct Path,
  Query-only, Path+Query, user-specified params untouched, no-extractor
  handlers unchanged.
- Confirm generated OpenAPI in an example carries the path/query params and
  the orval client types them.
