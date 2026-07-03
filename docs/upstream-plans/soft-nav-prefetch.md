# Soft Navigations Should Be Seeded Like Hard Loads

- **Reported-in:** react-todos (live demo review — "slight flicker" on list → detail)
- **Date:** 2026-07-03
- **Status:** fixed in 8b3dc48 (published in 0.3.4)

## Problem

The seeding convention covers exactly one case: hard loads. `prefetch.rs` runs
on the server, streams the cache, zero fetch on first paint. Soft navigation
never touches that path — the target page falls back to vanilla client-side
React Query: mount → cache miss → fetch → loading flash. The framework's
"no fetch on first paint" guarantee silently doesn't apply to the navigation
mode the framework itself made the default.

Symptom in the demo: soft-nav from the list to `/todos/1` flashed "Loading…"
(~8ms locally; a real network round-trip in production). Patched app-side with
`placeholderData` borrowed from the list's cache entry — a hand-written bridge
that shouldn't be the app's job and doesn't generalize (it requires another
page's cache to happen to contain the entity).

## Proposed Direction

Make the existing per-route prefetch logic reachable over the wire, and have
the app shell hydrate from it during navigation:

1. **Prefetch endpoint.** The generated registry already knows every route's
   `prefetch.rs`. Expose it: `GET /__nx/prefetch?path=/todos/1` runs the
   matched route's prefetch fn (params extracted from `path` exactly like a
   page render) and returns the entries as JSON — the same payload the
   `__nx_seeds__` tag would carry on a hard load.
2. **Shell integration.** The generated app shell's leaf routes get a TanStack
   `loader` that fetches the endpoint and `setQueryData`s each entry (skipping
   keys that already have fresh data). With `defaultPreload: "intent"` this
   runs on hover — by click time the cache is warm, and the leaf paints seeded
   with zero app code, matching hard-load behavior.
3. **Don't block the swap on it.** The loader should start the fetch but not
   gate rendering (or gate only within `defaultPendingMs`) — a slow prefetch
   must degrade to today's fetch-on-mount, never a frozen UI.

## Implementation Notes

- Routing: the endpoint needs the route table + prefetch fns, both already in
  the generated registry; matching `path` against the axum router internally
  (or a small matcher over the discovered patterns) yields the params.
- Skip routes without a prefetch fn (404 or empty array — cheap client check).
- Dedup: hovering N links shouldn't refire in-flight prefetches; TanStack's
  preload already dedups loader calls per route+params.
- Security posture is identical to the page itself: the endpoint runs the same
  server code a hard load of that path would run, after the same middleware.
- The demo's `placeholderData` patch in `todos/[id]/page.tsx` becomes
  unnecessary — remove it in the same PR as the living-reference proof
  (navigate list → detail with the network tab open: no todo fetch).

## Validation

- Router test: `/__nx/prefetch?path=...` returns the same entries the page's
  `__nx_seeds__` tag carries for static and dynamic routes; 404/empty for
  routes without prefetch.
- Browser e2e on react-todos (after removing the placeholder patch): hover a
  todo link, click — detail paints complete in one frame with no request for
  `/api/todos/{id}` at click time.
- Degradation: with the endpoint artificially slowed, navigation still swaps
  within `defaultPendingMs` and falls back to fetch-on-mount.
