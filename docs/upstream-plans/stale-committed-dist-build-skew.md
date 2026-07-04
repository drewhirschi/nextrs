# Stale committed dist — server/bundle build skew breaks pages in prod only

- **Reported-in:** nextrs-docs site (same symptom seen in other apps by Drew)
- **Date:** 2026-07-04
- **Status:** open   <!-- open | fixed in <commit> | wontfix -->

## Problem

A recurring failure class: an app works perfectly in dev but its React pages
never load in prod (blank page, no errors server-side). Confirmed live on
nextrs-docs.vercel.app: the landing HTML referenced `/dist/__app_shell__.js`
(emitted by a server built from post-0.3.2 framework source) while the
deployed `public/dist/` was a bundle committed to git months earlier — no
app shell file, so the script 404'd and React never booted.

Root cause: the server binary's HTML emitter and the JS bundle in
`public/dist/` MUST come from the same build — the emitter references bundle
files by name, and those names/architecture change across framework versions.
Dev always rebuilds both together, so it can never skew there. Any deploy
setup where the bundle is baked earlier than the server binary (committed
dist + `NEXTRS_SKIP_BUNDLE=1`, cached build layers) skews silently on the
next framework bump.

## Proposed Direction

Two parts:

1. **Kill the committed-dist pattern** (per-app fix): make each deploy
   self-sufficient — Vercel builds regenerate the client and the bundle,
   nothing generated is committed. Done for examples/react-todos in 5ebded3;
   the docs site gets the same treatment in this fix. Scaffolded apps already
   ship the self-sufficient vercel.json (create-nextrs-app 0.1.1).

2. **Fail loudly instead of blankly** (framework hardening, future): the
   server knows which dist files its HTML references. At startup (or first
   render) it could verify `public/dist/__app_shell__.js` exists and log an
   ERROR pointing at this exact skew, instead of serving HTML that 404s
   client-side. Tracked as a follow-up; not part of this fix.

## Implementation Notes

Site changes mirroring react-todos:

- `site/vercel.json`: drop `build.env.NEXTRS_SKIP_BUNDLE`; add
  `installCommand: "cd client && npm ci"` and
  `buildCommand: "cd client && npm run gen && cd .. && cargo build --release -p site"`.
- Untrack `site/public/dist/` and `site/client/src/generated/`; add
  `site/.gitignore` covering both (the `dump` script still sets
  `NEXTRS_SKIP_BUNDLE=1` — that use is fine, it only breaks the
  build.rs ↔ openapi chicken-and-egg for the dump binary).
- Update the stale comment in `site/.cargo/config.toml` that documented the
  old skip-bundle deploy.

## Validation

- Fresh local build after deleting `public/dist`: `cargo build -p site`
  produces `__app_shell__.js` + chunks; `curl /` references the shell and
  `/dist/__app_shell__.js` returns 200; `/docs/getting-started` renders.
- `cd site/client && npm run gen` (what Vercel's buildCommand runs) succeeds.
- After deploy: nextrs-docs.vercel.app landing page boots (shell 200, React
  renders) instead of the current 404.
