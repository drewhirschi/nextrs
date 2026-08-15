# Debug and release builds clobber each other's `public/dist`

- **Reported-in:** onenote-extractor
- **Date:** 2026-08-02
- **Status:** reported

## Problem

`bundle_pages` writes bundles and `nextrs-assets.json` into a single shared
`public/dist`, but debug and release builds emit *different* content hashes for
identical sources. In this app, the same app shell came out as
`__app_shell__-DRyvzfjo.js` under debug and `__app_shell__-tXPyCDj_.js` under
release.

A running server resolves asset names from the manifest as loaded at startup.
Any build in the *other* profile rewrites `public/dist` underneath it, and the
server then 404s its own app shell:

```
Failed to load script /dist/__app_shell__-DRyvzfjo.js
```

The concrete trigger here was the Playwright suite running a release build while
a `cargo dev` server was still serving debug assets. Nothing in the failure
points at the cause — the app breaks minutes after an apparently unrelated
command, in a browser tab nobody touched.

This is a direct consequence of content-addressed filenames
(`static-js-caching.md`), which are otherwise the right design. The hashes are
supposed to differ when content differs; the bug is that two profiles share one
output directory.

## Proposal

Any one of these closes it; the first is the most direct:

- **Per-profile output.** Write to `public/dist/{debug,release}` (or hash the
  profile into the manifest filename) so concurrent profiles cannot overwrite
  each other. Requires the server to resolve the manifest for the profile it was
  compiled in, which it already knows via `cfg!(debug_assertions)`.
- **Re-read the manifest per request in debug.** A running dev server then
  always serves whatever is currently on disk. Does not fix release-vs-release
  across two checkouts, but covers the common case.
- **Make bundle output byte-identical across profiles.** Cleanest if the hash
  divergence is incidental rather than semantic — worth confirming *why* the
  profiles differ before assuming this is achievable.

## Validation

- Build the same app under both profiles into one tree and assert the manifests
  no longer collide (or that a debug-profile server still resolves its own
  assets after a release build has run).
- Regression test for the reported shape: start a debug server, run a release
  build, request the page, assert 200 on the shell script rather than 404.
