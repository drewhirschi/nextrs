# Tailwind CSS goes stale while the TS bundle rebuilds

- **Reported-in:** onenote-extractor
- **Date:** 2026-06-23
- **Status:** reported

## Problem

`cargo build` / `cargo dev` rebuild the route bundle and serve updated JSX, but
do not regenerate `public/style.css`. New Tailwind utilities added to page
components — including arbitrary layout classes — appear in the compiled JS
while being absent from the CSS.

The browser then silently ignores those classes. The symptom is a component that
looks wrong, so the natural reaction is to go debug the React, when the actual
fault is stale CSS output from a build step that never ran. Nothing errors and
nothing warns.

The fix is to run `npm run css` from `client/`, which is documented — but only
if you already suspect CSS, which is exactly what the symptom argues against.

## Proposal

Pick one and make it explicit:

- **Drive the CSS build from the same pipeline.** Have the dev runner watch the
  Tailwind input plus `app/**/*.tsx` and regenerate `style.css` alongside the
  bundle. This is the least surprising behavior — one command, consistent
  output.
- **Or make the split loud.** If CSS generation stays deliberately outside
  nextrs, the dev runner should say so on startup and on rebuild ("bundle
  rebuilt; `style.css` not regenerated — run `npm run css`"), so the stale-CSS
  hypothesis is cheap to reach.

Complements `write-if-changed-generated-assets.md`, which addresses the churn
caused by *rewriting* `style.css` unnecessarily — the opposite failure. Both
want the CSS step modelled by the framework rather than left implicit.

## Validation

- Add a utility class to a page component, rebuild through the normal dev path,
  and assert the class is present in the served stylesheet.
- If the "make it loud" route is taken instead, assert the runner emits the
  notice when the bundle changed but `style.css` did not.
