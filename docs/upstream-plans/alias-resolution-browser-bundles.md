# Alias Resolution In Browser Bundles

## Problem

App code can import through the documented shadcn-style `@/*` alias, but a final
browser bundle may still contain raw unresolved imports such as
`@/components/ui/badge`. The browser then fails with `Failed to resolve module
specifier`, even though the build appeared to succeed.

## Proposed Direction

Alias resolution should be verified for every TSX convention entry:

- `app/page.tsx`
- `app/layout.tsx`
- `app/loading.tsx`
- files imported by those entries
- `client/src/*` files

The bundler must either fully resolve aliases or fail the build with a clear
diagnostic. It should not emit browser modules with unresolved app aliases.

## Implementation Notes

- Keep the built-in default alias `@/* -> <client_dir>/src/*`.
- Ensure alias handling applies to absolute entry wrapper imports generated
  under `$OUT_DIR/nextrs_tsx`.
- Inspect bundled output or bundler diagnostics for externalized specifiers.
- If an alias import is externalized, return a build error naming the import and
  the file that referenced it.

## Validation

- Add an integration test where `app/page.tsx` imports a component via
  `@/components/ui/badge`.
- Run the bundler and assert the emitted bundle does not contain raw `@/`.
- Add a negative fixture for an unknown alias and assert the build fails with a
  clear message rather than shipping invalid JS.
