# React Module Failure Fallback

- **Reported-in:** hand-assembled nextrs port; docs-site local dev (2026-07-18)
- **Status:** open — inline shell fallback shipped (incl. resource-load
  failures, see below); the React error boundary for post-mount render
  errors is still to do.

## Problem

If a generated TSX page module fails before React mounts, the browser can render
a blank page. One observed case was an unresolved browser import such as
`@/components/ui/badge`; the module failed before `createRoot(...).render(...)`,
leaving `#__nx_root__` empty.

**Second observed case (2026-07-18, docs-site local dev):** the app-shell
script itself 404s because the server binary and `public/dist` were produced
by different builds (a release/deploy build rewrote dist under a running
debug binary — content hashes differ per profile). The inline fallback that
already existed did NOT fire: resource load errors dispatch on the element
and never bubble to `window`, so a non-capture `window.addEventListener("error")`
never sees them. Fixed by listening in the capture phase and branching on
`event.target.src/href` to render a "Failed to load script: <url>" panel that
names the binary/dist-skew cause and the remedy. Regression test:
`tsx_shell_error_fallback_catches_resource_load_failures` in build.rs.

## Proposed Direction

Add a tiny development fallback around generated TSX shells:

- Inject an inline script before the page module script.
- Listen for `window.error` and `window.unhandledrejection`.
- If `#__nx_root__` is still empty, render a visible diagnostic panel into it.
- Include the error message and stack when available.
- Keep production behavior configurable. A visible fallback is useful in dev,
  but production may want either a minimal message or no stack.

Generated React entry wrappers should also wrap the page tree in a React error
boundary so render-time errors after module load are visible.

## Implementation Notes

- The shell fallback catches load/evaluation failures before React starts.
- The React error boundary catches component render failures after the module
  successfully imports.
- The fallback should be small and dependency-free because it sits inline in the
  server shell.

## Validation

- Add a build/router test that the TSX shell includes the inline error fallback
  before the module script.
- Add a browser-oriented integration test with a page importing a missing module
  and assert visible error text appears instead of an empty `#__nx_root__`.
- Add a React render-error fixture and assert the generated boundary renders a
  visible fallback.
