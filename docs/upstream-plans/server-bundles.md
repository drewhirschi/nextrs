# Route and subtree server bundles

- **Reported-in:** framework design discussion
- **Date:** 2026-09-05
- **Status:** fixed in d9ea5f2; docs deployment follow-up in the same PR

## Problem

One executable makes every function carry heavy route dependencies and runtime
resources. Docs production can also lag framework changes because Git cloud
builds are disabled and prebuilt deploys have been manual.

## Proposed Direction

Colocated `bundle.toml` assigns a subtree to a named function or isolates a single
endpoint. Build each with explicit Cargo features, generate routing from the same
ownership plan, and publish the complete artifact set together. Cross-bundle
prefetch is deferred, with an explicit diagnostic for unsupported placement.

## Implementation Notes

The CLI and build-time codegen share `server_bundles::BundlePlan`. Filtering clears
excluded Rust slots before module emission and preserves indices for generated
seed aliases. Deployment emits native executable functions with streaming support,
static assets, cron paths, and a route table ordered by path specificity.

The docs site uses an exact version plus workspace path dependency, reports the
linked framework version and build revision, and gains a CI-gated prebuilt deploy
job. `scripts/deploy-prebuilt.sh` delegates to the checkout's CLI so script and
CLI packaging cannot drift.

## Validation

Unit tests cover declaration inheritance/overrides/isolation, invalid assignments,
module exclusion and route precedence. Integration smoke builds the example's
separate executables and checks HTTP routing, resources and dependency exclusion.
Native and optimized Vercel executable adapters both pass the HTTP smoke.
The normal app builds and browser smoke retain frontend coverage. The docs deploy
job verifies the deployed framework version and revision before reporting success.
