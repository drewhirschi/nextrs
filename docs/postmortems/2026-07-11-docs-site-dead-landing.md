# Postmortem: docs-site landing page dead in production

- **Date discovered:** 2026-07-11 (user report, live site)
- **Symptom:** every load of nextrs-docs.vercel.app's landing threw
  `TypeError: Failed to resolve module specifier "@tanstack/react-router"` —
  React never mounted.
- **Fixed by:** 30e9b86 (site dependency) + 7f3f7dc (framework guard,
  published as nextrs 0.3.6).

## What the user saw

The served `/dist/__app_shell__.js` contained a literal
`import { ... } from "@tanstack/react-router"`. Browsers cannot resolve bare
specifiers in native ES modules, so the shell threw before mounting anything.

## The causal chain (three layers, all needed)

1. **Missing dependency.** When the app shell became how every `page.tsx`
   boots (PR #25), `@tanstack/react-router` was added to the example's client
   and the scaffold template — but not to `site/client/package.json`. The
   docs site is an app too; it was treated as "just docs."
2. **Silent externalization.** Rolldown ERRORS on unresolved relative imports
   but only WARNS on unresolved bare specifiers, externalizing them into the
   output. `bundle_pages` discarded `bundler.write()`'s warnings entirely, so
   the site's self-sufficient Vercel build (PR #31) went green while emitting
   a module guaranteed to fail at runtime.
3. **No production verification.** Example *previews* were verified
   repeatedly and thoroughly during this period; docs *production* — which
   auto-deploys from every push to main — was never loaded after the merges
   that changed how its pages boot. The landing was likely already broken
   between #25 and #31 in a different mode (the registry referenced
   `/dist/__app_shell__.js`, which the then-committed dist didn't contain),
   and nobody would have noticed that either.

## Why the process didn't catch it

- **The living-reference rule names react-todos, not "every app in the
  repo."** Framework changes were faithfully demonstrated and verified in the
  example — the rule worked — but the docs site sits outside its scope while
  depending on the same conventions via a path dependency, picking up every
  framework change immediately and silently.
- **Green builds were trusted as deploy verification.** The failure mode
  (warning-level diagnostic, discarded) was invisible to every check we ran:
  cargo tests, tsc, local browser e2e of the example — none of them load the
  docs site's bundle.

## Fixes shipped

- `site/client` gets the dependency; prod redeployed and verified.
- `bundle_pages` now fails the build on any `UNRESOLVED_IMPORT` warning, with
  the actionable message ("add it to <client>/package.json and npm install"),
  and surfaces all other rolldown warnings as `cargo:warning` instead of
  dropping them. Verified by reproducing the outage locally (dependency
  removed → build fails; restored → green). Published as 0.3.6, so every app
  gets the guard on its next rebuild — this failure class is now impossible
  to ship silently.

## Follow-ups (the remaining layer)

- [x] **Post-deploy smoke check for docs prod** — done, and generalized into
  a test suite (fa68d70 + 7165d25): `.github/workflows/ci.yml` (the repo had
  no CI at all) runs workspace tests, builds both apps with bundling ON, and
  browser-smokes every route of site and react-todos on each PR/push
  (`e2e/smoke.mjs`: fails on page errors, console.error, failed requests,
  empty React mounts, and bare imports in served JS). Pushes to main also
  smoke the live docs deployment (`e2e/prod-smoke.mjs`). The bundler guard
  is pinned by `crates/nextrs/tests/bundle_guard.rs`.
- [x] Widen CLAUDE.md's living-reference rule: `site/` is an app — framework
  behavior changes must be verified against it too, at minimum by building
  it with the bundler on (its skip-bundle dev path hides bundling breakage).
