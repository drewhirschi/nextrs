# The dev watcher interrupts Cargo builds and forces dependency rebuilds

- **Reported-in:** onenote-extractor
- **Date:** 2026-06-22
- **Status:** reported

## Problem

During `cargo dev`, file changes trigger a restart while the child command
`cargo run --bin <app>` is still compiling. The watcher kills the active Cargo
process and immediately spawns another one.

The cost is not the lost compile — it is that interrupting Cargo mid-write
invalidates fingerprints. Subsequent output showed expensive dependencies
recompiling that had nothing to do with the edit: `ring`, `rustls`,
`aws-runtime`, `aws-sdk-*`. A plain `cargo build -vv` after stopping the churn
reported the cause:

```
Dirty ring v0.17.14: the env variable CARGO_MANIFEST_DIR changed
```

which then cascaded through the whole TLS/AWS dependency graph. The next
identical build was clean, confirming the invalidation was an artifact of the
interruption rather than a real change.

The user-visible effect is that a one-line edit can cost minutes of rebuilding,
and it looks like a Cargo bug rather than a watcher behavior.

A related burst source: codegen and dependency edits rewrite `Cargo.lock` and
`client/src/generated/**` while the watcher is running, so a single logical
operation (`npm run gen`, adding a dependency) produces several restarts.

## Proposal

- **Do not kill an in-progress Cargo build on every change.** Coalesce changes
  and restart only after the current build completes, or after a longer quiet
  period. Debounce should span codegen/build bursts, not just consecutive
  keystrokes.
- **Separate compile from run**, so a restart kills only the app binary and
  never Cargo while it is writing artifacts. This removes the failure mode
  rather than reducing its frequency.
- **Ignore known generated artifacts** (`client/src/generated/**`, `Cargo.lock`)
  unless the app opts into watching them.
- **Document the fingerprint-invalidation symptom** so an interrupted build's
  cascading rebuild is not mistaken for a Cargo or dependency problem.

## Validation

- Drive edits into a running watcher during a cold compile and assert Cargo is
  not signalled until the in-flight build settles.
- Assert that a burst of writes across generated paths produces one restart
  rather than several.
