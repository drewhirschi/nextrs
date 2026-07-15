# Dev Static Asset Caching

- **Status:** fixed in this change

## Problem

During development, the browser could keep running an old generated asset
after the bundle changed. Freshness metadata such as `last-modified` still
allowed conditional reuse when generated JavaScript and CSS should be
unambiguously uncached.

## Implemented

Successful `/dist/*` responses from `build_router_with_public` now use:

```text
Cache-Control: no-store
```

in debug builds. Release builds use content-addressed URLs and:

```text
Cache-Control: public, max-age=31536000, immutable
```

Non-generated files under `public/` retain their existing behavior. Deployment
CDNs that bypass Axum must set the release header in platform configuration;
the Vercel examples and generated scaffold do so.

## Validation

- Router test: a successful generated asset has `no-store` in debug mode.
- Policy test: the release value is one-year immutable.
- Bundle tests and app builds verify that generated URLs change with content.
