# Static JS caching — no content hashes, no cache headers

- **Reported-in:** nextrs-docs site / react-todos prod (Drew: ".js files load more than they should")
- **Date:** 2026-07-04
- **Status:** open   <!-- open | fixed in <commit> | wontfix -->

## Problem

Prod serves every `/dist/*.js` file with
`cache-control: public, max-age=0, must-revalidate`. ETags mean repeat loads
get 304s rather than full re-downloads, but the browser still makes a
revalidation round-trip per file per hard load — and on Vercel each of those
goes through the serverless function (latency + invocation cost).

Two framework gaps cause it:

1. `bundle.rs` emits stable filenames — `entry_filenames: "[name].js"`,
   `chunk_filenames: "chunks/[name].js"` — so files can't be cached
   immutably; a new deploy must be able to change their content in place.
2. The router's `ServeDir` (router.rs `build_route_table`) sets no
   `Cache-Control`, so Vercel's conservative default applies.

(style.css already has the right idea: `tsx_document_head` content-hashes it
into a `?v=<hash>` query param.)

## Proposed Direction

Standard content-addressed pattern:

- Bundle with hashed names: `[name]-[hash].js` / `chunks/[name]-[hash].js`.
  rolldown rewrites inter-chunk imports automatically. The HTML emitter needs
  to learn the hashed entry names — bundle_pages should write a small
  manifest (entry slug → hashed filename) that the generated registry / shell
  src lookup reads, replacing the hardcoded `/dist/__app_shell__.js`.
- Serve `/dist` with `cache-control: public, max-age=31536000, immutable`
  (tower-http `SetResponseHeaderLayer` scoped to the dist route, or a wrapper
  service around ServeDir). Other public/ assets keep etag-revalidate.
- Dev: hashes change on rebuild, which also fixes any dev staleness; the dev
  runner's livereload is unaffected.

Open questions: does the Vercel static-vs-function split ever serve dist
directly (then headers must come from vercel.json instead)? Today everything
routes through the function, so the layer approach works.

## Implementation Notes

- `crates/nextrs/src/bundle.rs`: filename templates + manifest emission;
  `app_shell_entry` references stay name-based (rolldown resolves).
- `crates/nextrs/src/build.rs`: `tsx_shell_with_src` reads the manifest for
  the shell's hashed name (build-time, baked into generated code — safe
  because dist and registry are emitted by the same build, see
  [[stale-committed-dist-build-skew]]).
- `crates/nextrs/src/router.rs`: immutable header on the dist service.
- react-todos demonstrates: verify prod headers show `immutable` and repeat
  hard loads make zero /dist requests.

## Validation

- Unit: bundle output filenames contain hashes; manifest maps slugs; HTML
  references the hashed shell.
- Browser: second hard load of the todo list issues no network requests for
  /dist files (memory/disk cache), where today each revalidates.
