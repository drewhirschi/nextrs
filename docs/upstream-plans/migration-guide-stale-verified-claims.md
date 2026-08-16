# Migration guide asserts obsolete conventions as "verified"

- **Reported-in:** hhh (via the .nextrs client migration follow-up)
- **Date:** 2026-08-15
- **Status:** fixed in 36d6c0b

## Problem

`docs/migrating-nextjs-to-nextrs.md` still claims — marked **verified** — that
`layout.tsx` and `loading.tsx` are not conventions (lines 84–85, §13 gap
table), prescribes hand-porting layouts to `layout.html` (§5.1, line 27
overview table), and lists `not-found.tsx` and client-side navigation as
framework gaps. All of these shipped:

- `layout.tsx` / `loading.tsx` recognized since 7e9b5bf (nextrs 0.3.7, 2026-06-27)
- `not-found.tsx` convention: PR #22
- soft navigation / seeded client-side nav: PR #29 (8b3dc48)

Because the claims carried an unqualified "verified", a downstream migration
propagated them confidently into app code comments (`layout.html` kept, a
shell.tsx workaround) instead of using `layout.tsx`.

## Proposed Direction

The guide is an archived playbook (banner already says so), so don't rewrite
the historical procedure. Instead: (1) extend the banner with a dated list of
claims known to have been invalidated, (2) correct the specific assertions
inline with dated update notes, (3) drop or timestamp "verified" phrasing so
version-specific findings don't read as permanent facts.

## Implementation Notes

Touched spots: banner, line-27 overview table, §1.1 rows (layout.tsx,
loading.tsx, not-found.tsx), §5.1 intro, §13 gap-table rows (layout/loading,
not-found, client-side nav).

## Validation

`rg -n "not a convention" docs/migrating-nextjs-to-nextrs.md` finds only
dated/corrected text; each corrected row names the shipping version or PR.
