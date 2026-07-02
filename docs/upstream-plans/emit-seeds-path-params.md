# `emit_seeds` Should Support `Path`-Param Routes

- **Reported-in:** onenote-extractor
- **Date:** 2026-07-01
- **Status:** fixed in 3e2d538

## Problem

`emit_seeds` only generates typed seed companions for GET handlers with zero
args or one `Query<T>` extractor. Every `Path`-param route
(`/api/sources/{id}`, `/{id}/pages`, `/{id}/regions`) is excluded. Seeding the
source detail page from `props.rs` therefore requires hand-rolling: parse the
id from `req.uri()`, query the DB directly, and construct `SeedEntry` with a
manually built `seed_key` that must match the generated hook's query key by
convention.

## Proposed Direction

Extend eligibility to `Path` extractors. The generated companion takes the
typed path params, substitutes them into the URL, and keys the entry exactly
like the generated client keys the same request:

- `get(Path(id): Path<i64>)` on `/api/sources/{id}/pages` →
  `__nextrs_seed_get(id: i64, _ext)` with
  `seed_key(&format!("/api/sources/{id}/pages"), None)`.
- Combined `Path` + `Query` handlers get both: typed path args plus the params
  struct, keyed `[substituted_url, params]`.

Combined with route params reaching `props.rs`, seeding a param'd page becomes
a few typed calls.

## Implementation Notes

- The orval client's query key for a param'd GET is the *substituted* URL
  (`` [`/api/sources/${id}/pages`] ``), so substitution must happen before
  `seed_key`.
- Extend both the macro's `seed_companion` and build.rs's textual mirror
  `get_is_seed_eligible`; supported shapes: `[]`, `[Query]`, `[Path]`,
  `[Path, Query]`.
- Path values substitute via `Display` — matches how they appear in a URL.

## Validation

- Macro tests: scalar Path, tuple Path, Path+Query companions; key shape
  matches the orval substituted-URL form.
- build.rs tests: eligibility mirror accepts the new shapes.
