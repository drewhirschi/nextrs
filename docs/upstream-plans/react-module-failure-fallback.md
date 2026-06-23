# React Module Failure Fallback

## Problem

If a generated TSX page module fails before React mounts, the browser can render
a blank page. One observed case was an unresolved browser import such as
`@/components/ui/badge`; the module failed before `createRoot(...).render(...)`,
leaving `#__nx_root__` empty.

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
