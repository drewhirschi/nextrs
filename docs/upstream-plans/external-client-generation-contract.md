# The external client config lives inside a directory the framework deletes

- **Reported-in:** linkedin-challenge (publishes a React-free client to a browser extension)
- **Date:** 2026-08-15
- **Status:** open

## Problem

`generate_client` looks for the external-client config at
`<client_dir>/nextrs.client.json`, and runs `npm run generate:external` **in the
client dir**. Under the current layout that directory is `.nextrs/client` — a
build product, gitignored, recreated from `.nextrs/template/` by
`client:ensure`.

So an app that publishes an external client has to put both the config and the
script it invokes in `.nextrs/template/client/`, and reason about every path
twice: once as authored in the template, once as materialized one level deeper.
`nextrs.client.json`'s `output` is resolved relative to the *materialized*
location, so a path that was `../../extension/...` under the old `client/` layout
becomes `../../../extension/...`. Nothing checks that, and the failure mode is a
generated client published to the wrong directory.

The rest of the contract is similarly implicit:

- **The generated package must carry a `generate:external` script**, because the
  CLI runs it there — even though the layout's stated intent is that the
  generated package owns no workflow and all scripts live at the app root. The
  script ends up as a shim that shells back to the root.
- **`cargo build` has to run between orval and `tsc`.** Orval's `clean: true`
  wipes `src/generated/`, including the barrel `index.ts` that `emit_barrel`
  writes. Compile without rebuilding it and both package entry points fail to
  resolve their own generated code:

  ```
  src/index.ts(2,15): error TS2307: Cannot find module './generated/fetch'
  ```

  The root `client:generate` script has this ordering, so a custom external
  generator that reuses the other `client:*` scripts silently omits the step it
  cannot see. The error names the package's own source file and reads like a
  broken template, not a missing build.

- **`ensure-client.mjs` refreshes a script that is already running.** The CLI
  invokes `generate:external` from `.nextrs/client/scripts/`, and that script's
  first act is `client:ensure`, which rewrites itself from the template. Node has
  already loaded the old copy, so an edit to the template takes effect only on
  the *next* invocation. Debugging a change that appears to do nothing, and then
  works unmodified on a second run, costs a cycle.

None of this is unreasonable once known; all of it was learned by reading
`cargo-nextrs/src/lib.rs`. The external client is the one documented extension
point for consumers outside the app, so it is worth being legible.

## Proposed Direction

- **Move the config out of the build product.** Read `nextrs.client.json` from
  the app root (or `.nextrs/`), resolving `output` relative to the app root — one
  stable location, one obvious base path, tracked by git without a template
  round-trip. Keep the current path as a fallback.
- **Run `generate:external` at the app root**, like every other `client:*`
  script, so the generated package keeps owning no workflow and the shim
  disappears.
- **Fold the external target into normal generation.** The extra work is one more
  orval target plus a `tsc -p`; if the config exists, the ordinary pipeline could
  run it as a final step rather than replacing the pipeline with an app-supplied
  script. That removes the chance of an app reimplementing the sequence and
  missing the `cargo build`.
- Failing that, ship the external generator as a template file the app does not
  edit, and document the ordering constraint where `clean: true` is configured.

## Validation

- An app whose `nextrs.client.json` sits at the app root publishes to the same
  directory as one using the legacy in-client path.
- Generation that goes through the external path produces a client package
  identical to one that does not — same `dist/`, same barrels.
- Deleting `.nextrs/client` and regenerating publishes the external client
  correctly on the first run, with no second invocation needed.
- A template edit to any `.nextrs/template/client/scripts/*` file takes effect on
  the first run after the edit.
