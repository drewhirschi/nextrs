# Content-addressed browser assets and immutable caching

- **Reported-in:** nextrs docs site / react-todos production
- **Date:** 2026-07-04
- **Status:** fixed in this change

## Problem

Production served every `/dist/*.js` file with
`cache-control: public, max-age=0, must-revalidate`. ETags prevented full
downloads, but the browser still paid a revalidation round trip per file per
hard load. The stylesheet used a query hash but retained a stable pathname, so
it also revalidated on the deployment CDN.

Two framework gaps caused it:

1. Rolldown emitted stable entry and chunk filenames, so a new deploy could
   change content at an existing URL.
2. Neither Axum's `ServeDir` nor the Vercel deployment templates installed a
   generated-asset cache policy.

## Implemented

- Rolldown emits `[name]-[hash].js` entries and
  `chunks/[name]-[hash].js` shared chunks.
- `bundle_pages` writes a manifest and generated Rust asset table. Page,
  loading, and not-found shells embed the exact resolved entry URLs.
- `public/style.css` is copied to `/dist/style-<content-hash>.css` and that URL
  is used in both React shells and server-rendered site layouts.
- Successful local `/dist/*` responses use `no-store` in debug and
  `public, max-age=31536000, immutable` in release.
- Vercel serves matching files under `public/` before the catch-all function,
  so the docs site, example app, and generated scaffold apply the immutable
  header in `vercel.json` too.
- `create-nextrs-app` emits the Vercel adapter and cache policy by default, so
  future applications inherit the production-safe configuration.

## Validation

- Unit tests cover the content-addressed stylesheet, generated shell URLs, and
  debug/release cache policies.
- Site and example builds verify the generated manifest and compiled asset
  table against real Rolldown output.
- Production follow-up: verify repeat hard loads make zero conditional
  `/dist/*` requests and compare HAR timing against the pre-change capture.
