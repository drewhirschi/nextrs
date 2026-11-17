# Vercel builds on every push because "git deploys off" lives in a gitignored file

- **Reported-in:** site (nextrs-docs)
- **Date:** 2026-11-16
- **Status:** open   <!-- open | fixed in <commit> | wontfix -->

## Problem

Every push to the nextrs repo starts a Vercel cloud build of the docs site,
and it always fails:

```
nextrs::bundle::bundle_pages failed: Custom { kind: NotFound, error: "cannot resolve generated client d…
```

We had already fixed this once. [build-times-and-deploy-reliability](build-times-and-deploy-reliability.md)
turned Git auto-builds off with `"git": { "deploymentEnabled": false }` in
each app's `vercel.json`. PR #41 (fc8112b) then moved Vercel configuration to
a generated `.nextrs/vercel.json`, gitignored and passed to the CLI with
`--local-config`, and deleted the committed root file. The setting is still
rendered into the generated file — where Vercel's Git integration, which
reads only the committed root `vercel.json`, never sees it. So the
integration fell back to building on push, and the cloud build cannot
succeed because the generated client is not in git either.

This affects every app generated or migrated since PR #41, not just the docs
site: `git_deploys = false` (the default) has silently done nothing.

## Proposed Direction

Make the framework own a committed root `vercel.json` that carries only the
Git setting. `nextrs generate` writes it when `git_deploys` is false and the
file is missing, removes it when `git_deploys` is true and the file is still
the stub, and never touches a hand-written file. The scaffold emits the same
stub so a new app is safe before its first `generate`.

## Implementation Notes

- The stub text is `create_nextrs_app::VERCEL_GIT_STUB`; `cargo-nextrs`
  reuses it (`cron.rs`: `sync_git_stub`, `is_git_stub`).
- "Is it the stub" is strict: `git.deploymentEnabled: false` plus optionally
  `$schema`, nothing else. Anything more is treated as hand-written.
- The existing legacy-file warning stays for hand-written files and now says
  the Git integration does read the file.
- `nextrs deploy` is unaffected: it passes `.nextrs/vercel.json` with
  `--local-config`.
- Existing apps pick this up on their next `nextrs generate` / `nextrs
  deploy`, and must commit the new file.
- Not addressed: with `git_deploys = true` the cloud build still needs a
  hand-written root `vercel.json` carrying the commands; the docs now say so.

## Validation

- `cargo test -p cargo-nextrs`: `generate_keeps_the_git_stub_in_step_with_git_deploys`,
  `git_stub_is_recognised_strictly`.
- `cargo test -p create-nextrs-app`: both template sets ship the stub.
- On the PR that adds `site/vercel.json`, Vercel must not start a build.
