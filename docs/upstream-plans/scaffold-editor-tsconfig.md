# Scaffolded apps have no tsconfig an editor can find

- **Reported-in:** linkedin-challenge (adopted via `create-nextrs-app --adopt`)
- **Date:** 2026-08-09
- **Status:** fixed in the scaffold; existing apps should migrate as described below

## Problem

`create-nextrs-app` writes exactly one `tsconfig.json`, at `client/tsconfig.json`:

```json
{
  "compilerOptions": { "paths": { "@app/client": ["./src/index.ts"] } },
  "include": ["src", "../app/**/*.tsx"]
}
```

It reaches *down* into `../app/**/*.tsx`, so `npm run typecheck` covers the pages
and passes. But an editor does the opposite: it resolves an open file against the
**nearest tsconfig above it**. Opening `app/orgs/[slug]/page.tsx` walks up through
`app/`, the app root, and the repo root, finds nothing, and falls back to
no-project mode.

The result, on every `.tsx` file in a scaffolded app:

- `import { … } from "@app/client"` — **Cannot find module**. The alias is only
  declared in a config the editor never loaded.
- Every callback parameter is an implicit `any`: `posts.map((post) => …)` shows
  `post: any`. No autocomplete on generated hooks, no error when a field is
  renamed in Rust.
- No JSX typing, so `className`, props, and children are unchecked.

This is the sharp edge: **`npm run typecheck` passes while the editor shows the
whole app as untyped**, so it reads as a broken toolchain rather than a missing
config. The typed client is nextrs's headline feature — a Rust field rename
should light up at the call site — and out of the box it does so only in CI. The
person most likely to hit this is someone new to the framework, on day one, with
no reason to suspect `client/tsconfig.json` is the cause.

## Resolution

Scaffold one canonical `tsconfig.json` at the **app root** and remove the
ordinary `client/tsconfig.json`:

```jsonc
// tsconfig.json  (app root — the one an editor finds first)
{
  "compilerOptions": {
    // …same options…
    // Resolved relative to THIS file, so it must reach into client/.
    "paths": { "@app/client": ["./client/src/index.ts"] }
  },
  "include": ["app/**/*.tsx", "app/**/*.ts", "client/src"]
}
```

The client package runs `tsc --project ../tsconfig.json`. The specialized
`client/tsconfig.external.json` remains separate because it emits the portable
external client and serves a different purpose.

Verified in both repository apps: the root project typechecks cleanly when run
from `client/`, and every page resolves the same project. The package install's
root `node_modules` link makes React's types available without restricting
ambient type discovery through `typeRoots`.

Two details worth keeping in the template:

- Paths move from `./src/index.ts` to `./client/src/index.ts` because they are
  now resolved relative to the app root.
- `client/src/generated/index.ts` is written by `cargo build` and deleted by
  `npm run gen`. Between those two steps the alias resolves to a barrel that
  exports nothing, so the editor reports every hook as missing. Worth a line in
  the scaffold's `AGENTS.md`: **run `cargo nextrs client generate`, then
  `cargo build`, before trusting editor diagnostics.**

## Why not just document it

A comment in `client/tsconfig.json` would not be found by someone whose symptom
appears in `app/**/page.tsx`. The config has to exist where the editor looks.

## `--adopt` needs care

`--adopt` promises never to overwrite an existing file, which the root config
must respect:

- Write `tsconfig.json` when none exists.
- Leave an existing one untouched.
- Print explicit merge instructions when one exists: the alias, the `include`
  entries, and the required compiler options.

## Existing apps

Move the compiler options from `client/tsconfig.json` into a root
`tsconfig.json`, change client-relative aliases to root-relative paths, include
both `app/**/*.ts(x)` and `client/src/**/*.ts(x)`, and change the client package's
typecheck script to `tsc --project ../tsconfig.json`. Do not add `typeRoots`
unless the normal root `node_modules` link is unavailable; setting it restricts
ambient type discovery.
