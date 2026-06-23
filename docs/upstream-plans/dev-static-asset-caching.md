# Dev Static Asset Caching

## Problem

During development, the browser can keep running an old `/dist/index.js` after
the bundle changes. Static serving currently exposes freshness metadata such as
`last-modified`, but generated JS/CSS should be unambiguously uncached in dev.

## Proposed Direction

Serve generated development assets with:

```text
Cache-Control: no-store
```

This should apply at least to generated TSX assets under `/dist/*` while running
the development server. A future production story can use content-hashed URLs or
long-lived immutable caching.

## Implementation Notes

- Prefer dev-only `no-store` first. It is simpler than build counters and avoids
  touching every generated shell URL.
- Keep production static asset caching separate from dev behavior.
- If no explicit dev mode flag exists in the router/static layer, add a small
  config surface rather than guessing from `debug_assertions` in library code.

## Validation

- Add a router/static asset test that `/dist/index.js` has
  `Cache-Control: no-store` in dev mode.
- Confirm non-generated public assets keep their current behavior unless the app
  opts into broader no-store.
- Manual check: edit a TSX page, rebuild, refresh, and confirm the browser loads
  the new bundle without cache intervention.
