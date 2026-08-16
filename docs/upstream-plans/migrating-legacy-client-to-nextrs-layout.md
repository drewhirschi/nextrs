# Existing apps have no path from `client/` to `.nextrs/client`

- **Reported-in:** linkedin-challenge (adopted pre-0.5.1, migrated by hand)
- **Date:** 2026-08-15
- **Status:** open

## Problem

[Generated client package resolution](generated-client-package-resolution.md)
moved the generated client to `.nextrs/client`, owned by the framework and
materialized from a tracked `.nextrs/template/`. New apps get that from
`nextrs new`. Apps scaffolded before it get nothing — the CLI detects the old
layout and says so:

```
nextrs: using legacy client directory client; regenerate the app scaffold to
move it to .nextrs/client
```

"Regenerate the app scaffold" is not a command. `nextrs new` refuses a non-empty
directory, and `--adopt` never overwrites, so neither produces the new wiring for
an app that already exists. The fallback keeps working, which is the right call,
but it means the only way onto the current layout is to reconstruct it by hand.

Doing that meant copying nine files out of `examples/react-todos/.nextrs/` —
`ensure-client.mjs`, `dump-openapi.rs`, and the whole `template/client/` tree
(`package.json`, `tsconfig.json`, `orval.config.ts`, `scripts/normalize-esm.mjs`,
`src/{index,react-query,nextrs-client}.ts`) — and then editing each for the app's
own package name. The example is load-bearing documentation and nothing says so.

The parts that are not copy-paste, and that a migrating app has to work out:

- **The root `package.json` may not exist at all.** The old scaffold put the
  only `package.json` in `client/`, with a `postinstall` symlinking
  `client/node_modules` upward. The new layout needs a root manifest declaring
  `workspaces: [".nextrs/client"]` and a `file:./.nextrs/client` dependency —
  the contract `validate_root_client_contract` enforces, but only after you have
  guessed it correctly.
- **The client package name is a rename across the app.** `@app/client` becomes
  whatever the root manifest declares, and every page import plus
  `BundleConfig::client_alias` has to move with it. Splitting into
  `<pkg>` and `<pkg>/react-query` means sorting existing imports by whether each
  name is a hook or a wire type.
- **App code hiding in the client package has to come out first.** The old
  `client/src/index.ts` was a seam apps were told to edit — ours held display
  helpers and a `useSeed` hook alongside `export * from "./generated"`. In the
  new layout that file is template-owned and regenerated, so anything
  hand-written there is silently destroyed on the first `client:ensure`. Nothing
  warns about this, and the loss is invisible until a page fails to resolve an
  import.
- **`.gitignore` inverts.** The old layout tracked `client/src/generated/**`;
  the new one ignores `/.nextrs/client/` and `/.nextrs/openapi.json` and tracks
  the template instead. An app that misses this commits a build product into a
  directory the framework recreates.
- **`dump-openapi` moves and is renamed.** From `src/bin/dump-openapi.rs`
  writing `client/openapi.json` to `.nextrs/dump-openapi.rs` writing
  `.nextrs/openapi.json`, with a `<pkg>-dump-openapi` bin target. react-todos
  reaches the registry through its `[lib]` (`src/app.rs`); apps whose library is
  a domain layer and whose router lives in `main.rs` need the `include!` form
  instead. Both work; the difference is unexplained.

Separately: `cargo install cargo-nextrs` from an older release still resolves
`./client` and fails on a migrated app with
`<root>/./client/package.json does not exist`. The message names a path the app
deliberately no longer has, and does not mention the CLI's own version. Anyone
who migrates before reinstalling the CLI sees this.

## Proposed Direction

A `nextrs client migrate` that takes an app on the legacy layout to the current
one, or `--adopt` extended to recognize and upgrade an existing nextrs app
rather than only filling gaps in a non-nextrs one. It has enough information to
do nearly all of it:

- Write `.nextrs/ensure-client.mjs`, `.nextrs/dump-openapi.rs`, and
  `.nextrs/template/client/` from the same source `nextrs new` uses, substituting
  the package name.
- Create or amend the root `package.json` with the workspace entry, the `file:`
  dependency, and the `client:*` scripts; drop the `postinstall` symlink.
- Rewrite `build.rs` (`client_dir`, `client_alias`, `project_dir`) and the
  `dump-openapi` bin target.
- Update `.gitignore` both ways.
- Report, without touching, the two things it cannot decide: hand-written code in
  `client/src/index.ts` that needs a home in app code, and the import rewrite
  across `app/**` including the hook/type split.

Failing the command, a migration guide in `docs/` that states the file list, the
root-manifest contract, and the `client/src/index.ts` data-loss hazard would
cover most of it. The hazard deserves a hard check regardless: `client:ensure`
overwriting a tracked file that differs from the template should refuse rather
than clobber.

## Validation

- A fixture app on the legacy layout migrates and then passes
  `nextrs client generate`, `tsc`, and `cargo check --all-targets`.
- Migration is idempotent — running it twice changes nothing the second time.
- An app with hand-written exports in `client/src/index.ts` gets a report naming
  them, and the file is not silently replaced.
- The legacy-path error in `generate_client` names the CLI version and points at
  the migration command instead of "regenerate the app scaffold".
