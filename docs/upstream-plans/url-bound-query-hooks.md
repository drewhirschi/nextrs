# URL-Bound Query Hooks (`useXFromUrl`) + Search Params in `prefetch.rs`

- **Reported-in:** design discussion (search-params/URL-state, 2026-07-03)
- **Date:** 2026-07-03
- **Status:** fixed in HEAD (feat/url-bound-hooks, hash on merge)

## Problem

The typed-hook pattern quietly encourages unshareable pages: filter/pagination
state lives in `useState`, the hook argument follows it, and the page URL never
reflects it. Share the link and the recipient sees something different. nuqs
solves the ergonomics of URL state generically, but makes you hand-declare
parsers (`parseAsString.withDefault(...)`) for information the OpenAPI spec
already has — the Rust `Query<T>` extractor declared the names and types.

Separately, `prefetch.rs` has no ergonomic access to the request's search
params, so seeding a `?status=done` page means hand-parsing `req.uri().query()`.

## Proposed Direction

**Convention: a page's search params are the query params of its data.** Same
names, same types, typed by the spec.

1. **Generated URL-bound hook variants.** For each GET with query params, the
   client codegen (post-orval step, same home as `gen-barrel.mjs`) emits a
   sibling wrapper:

   ```tsx
   const { data, params, setParams } = useGetTodosFromUrl();
   ```

   - Reads the page's search params live from the app-shell router
     (`useSearch`), coerces them per the OpenAPI types, and feeds them to the
     underlying orval hook — so the React Query key is derived from the URL.
   - `setParams(patch)` performs `router.navigate({ search })` — a soft
     navigation. URL updates → hook re-keys → fetch (or instant cache hit for
     previously-visited states; back/forward walks warm cache entries).
   - Opt-in per call site: the plain `useGetTodos(params)` stays for computed
     params. `useGetTodosFromUrl({ fixed: {...} })` mixes URL-bound and fixed.
   - Setter takes a history option (`push` default for filters, `replace` for
     typeahead-style updates).

2. **Search params reach `prefetch.rs`.** A helper parses the request's query
   string into the handler's typed params struct
   (`nextrs::search_params::<TodosFilter>(&req)`, serde_urlencoded under the
   hood), so a hard load of `/todos?status=done` seeds exactly the key the
   URL-bound hook derives. Server knowledge and client knowledge are the same
   knowledge, keyed identically.

3. **nuqs stays a documented option** for URL state that is NOT an API param
   (selected tab, open panel) — recommended pattern, not a core dependency.

## Implementation Notes

- Codegen input is `client/openapi.json` (already produced by `npm run dump`);
  emit `src/generated/url-hooks.ts` (or per-tag) exporting the wrappers, and
  include it from the generated barrel.
- Type coercion comes from the spec's parameter schemas (string/number/bool/
  optional). Unknown/extra search params pass through untouched — pages may
  also carry nuqs-style UI state in the same URL.
- Name collisions (two hooks on one page reading `?page=`) collide by design —
  shared URL namespace. Revisit with a prefix option only if it bites.
- The wrapper imports `useSearch`/`useNavigate` from `@tanstack/react-router`
  (already a client dep as of the app-shell release).
- Demo: make the react-todos open/all filter URL-driven
  (`/?status=done` shareable, seeded on hard load, soft-updated by the select).

## Validation

- Codegen test: spec with string + number + optional params → wrapper coerces
  and defaults correctly; fixed-params merge; barrel exports it.
- `search_params::<T>` unit tests: present/absent/partial/junk query strings.
- Browser e2e on react-todos: set filter → URL updates without document load;
  back button restores previous filter from cache without a fetch; hard load
  of the filtered URL renders seeded (no mount fetch); shared URL reproduces
  the exact view.
