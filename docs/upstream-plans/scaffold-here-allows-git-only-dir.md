# `--here` refuses a directory containing only `.git`

- **Reported-in:** onenote-extractor
- **Date:** 2026-06-20
- **Status:** reported

## Problem

Running `create-nextrs-app --here` in a freshly initialized repository fails:

```
error: . already exists and is not empty
```

The directory contained only `.git` (plus, in this case, two markdown files).
Removing the markdown files was not enough — `.git` alone still tripped the
check. Generation only succeeded after moving `.git` aside, which is an
uncomfortable thing to ask someone to do to a repository.

"Create the repo, then scaffold into it" is a normal order of operations, and it
is the one that this check rejects. The error also names no offending entries,
so the obvious reading — "the directory looks empty to me" — is a dead end.

Distinct from `--adopt` (see `porting-paved-road.md`), which targets an existing
*application*. This is about an empty repo with no app in it yet.

## Proposal

- Treat a VCS-only directory as empty by default: ignore `.git` (and reasonable
  siblings like `.gitignore`, `LICENSE`, `README.md`) when deciding whether
  `--here` may proceed.
- Add an explicit `--force` / `--allow-non-empty` for the genuinely non-empty
  case.
- Make the error list the actual blocking entries, so "not empty" is
  actionable rather than a riddle.

## Validation

- Generator tests for: a directory containing only `.git` (succeeds), a
  directory with an unrelated source file (fails, and the message names that
  file), and the same with `--force` (succeeds).
