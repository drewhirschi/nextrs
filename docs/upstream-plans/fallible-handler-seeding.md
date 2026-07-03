# Seed Companions for Fallible Handlers (`Result<Json<T>, E>`)

- **Reported-in:** onenote-extractor
- **Date:** 2026-07-03
- **Status:** open

## Problem

The seed-companion macro only fires for handlers returning `Json<T>` directly.
Every real route returns `Result<Json<T>, ApiError>` — the moment a handler
grows a `?` on a DB call it silently drops out of seeding, so production pages
fetch-on-mount with a loading flash while toy handlers seed fine.

Worse, the two eligibility gatekeepers disagree: the macro parses structurally
(`last segment == "Json"` → rejects `Result<...>`), while build.rs's textual
mirror uses `ret.contains("Json")` → accepts it. A `Result`-returning handler
with companion-shaped args gets an alias emitted into the generated seeds file
for a companion that was never generated — "cannot find `__nextrs_seed_get`"
at the `include!` site. Whether a user sees silent ineligibility or a
confusing build break depends on their argument shapes.

## Proposed Direction

1. **Widen the macro**: `Result<Json<T>, E>` handlers get a companion that
   returns `Option<SeedEntry>` — `Ok` seeds, `Err` yields nothing and the page
   degrades to fetch-on-mount, where the hook surfaces the error exactly as it
   would have anyway. `QuerySeed::seed` accepts both shapes via a small
   `IntoSeedEntries` trait (`SeedEntry`, `Option<SeedEntry>`) — backward
   compatible.
2. **Fix the mirror**: replace `contains("Json")` with a normalized prefix
   match accepting only `Json<...>` / `Result<Json<...>, _>` heads (path
   qualifiers allowed), so the two gatekeepers agree by construction.
3. **Demo**: convert the react-todos detail GET to
   `Result<Json<TodoDetail>, StatusCode>` (404 on unknown id) — the living
   reference proves fallible seeding end to end, including the miss path.

## Explicit boundary (documented, not fixed here)

- `impl IntoResponse` returns stay unseedable — opaque to the macro. Guidance:
  seedable GETs declare concrete `Json<T>` / `Result<Json<T>, E>`.
- Type aliases (`ApiResult<Json<T>>`) are invisible to both checks — spell the
  return out.
- Handlers taking `State<..>`/`Extension<..>` stay companion-ineligible — the
  companion can't conjure those values. Follow-up:
  [[state-extractor-seeding]].

## Validation

- Macro tests: `Result<Json<T>, E>` companions for zero-arg / Query / Path /
  Path+Query shapes; `Err` produces no entry; plain `Json<T>` unchanged.
- seed.rs tests: `.seed()` accepts both `SeedEntry` and `Option<SeedEntry>`
  futures; `None` adds nothing.
- Mirror tests: agreement with the macro across `Json`, `Result<Json,..>`,
  `Result<T, Json<E>>` (rejected), `impl IntoResponse` (rejected),
  `axum::Json` (accepted).
- Browser e2e on react-todos: detail page seeded via the fallible handler on
  hard load; unknown id renders the miss state without a build error.
