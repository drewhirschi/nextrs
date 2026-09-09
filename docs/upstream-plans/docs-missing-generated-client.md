# Docs cloud build: missing generated client directory

- **Reported-in:** nextrs docs PR preview
- **Date:** 2026-09-07
- **Status:** diagnostic fixed in b072f01; deployment configuration remains open

## Problem

The docs cloud preview called Cargo before preparing the generated client.
`site/.nextrs/client` is intentionally untracked, and `bundle_pages` canonicalized
it with a bare `?`, producing OS error 2 without identifying the missing path.
Vercel project inspection shows default install/build commands; the generated
`.nextrs/vercel.json` is not a tracked `site/vercel.json` consumed automatically
by Git builds. The documented prebuilt workflow and actual Git build differ.

## Proposed Direction

Report the role, absolute path, original error kind, and recovery hint when a
bundling input directory cannot be resolved. Separately align the cloud build
with client preparation or enforce the intended prebuilt deployment workflow;
do not treat a better error as fixing deployment configuration.

## Implementation Notes

`resolve_bundle_directory` adds context for app, generated-client, and project
directories. Generated-client failures recommend `npm run client:prepare`.
No Vercel project settings were changed and no deployment was performed.

## Validation

A regression asserts the missing path, role, recovery hint and preserved
NotFound error kind. All 37 bundler unit tests pass. Temporarily moving the docs
client aside reproduced the failure with the full path in the new diagnostic;
the client was restored automatically afterward.
