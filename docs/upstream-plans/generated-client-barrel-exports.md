# Generated Client Barrel Exports (Both Flavors Should Be Visible)

- **Reported-in:** onenote-extractor
- **Date:** 2026-07-01
- **Status:** fixed in 30d5806

## Problem

Orval already generates framework-free typed clients alongside the React Query
hooks (`getSources()`, `updateSource(id, body)`, URL builders) — usable in
event handlers, scripts, and tests. In the app they went unused: the scaffolded
`client/src/index.ts` re-export list is hand-maintained and went stale, so
whole generated tag modules (`pages`, `regions`, `ocr`, `image`) were never
exported, and the page fell back to 11 raw `fetch()` calls with 6
hand-duplicated types.

## Proposed Direction

- Generate the barrel from the same codegen pass: after orval runs, write
  `src/generated/index.ts` re-exporting every tag module plus `model`. The
  scaffolded `src/index.ts` then re-exports `./generated` once and never goes
  stale as endpoints are added.
- Document that codegen produces both flavors — hooks for components, plain
  typed clients for everything else — so app authors reach for them instead of
  raw `fetch`. (Related: the typed-client docs already note the plain-fetch
  flavor should not require React Query.)

## Implementation Notes

- Orval `tags-split` emits `<tag>/<tag>.ts` per tag and `model/index.ts`, but
  no root barrel — a small post-orval step in the scaffold's `gen` script
  (node one-liner listing `src/generated/*/`) closes the gap.
- Keep hand exports (components, helpers) in `src/index.ts`; only the
  generated section becomes automatic.

## Validation

- Scaffold test: generated package.json `gen` script includes the barrel step;
  generated `index.ts` re-exports `./generated`.
- In a scaffolded app: add a second route, re-run `npm run gen`, confirm the
  new module is importable from the client package without editing index.ts.
