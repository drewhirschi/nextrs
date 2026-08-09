# Scaffolded apps have no tsconfig an editor can find

- **Reported-in:** linkedin-challenge (adopted via `create-nextrs-app --adopt`)
- **Date:** 2026-08-09
- **Status:** worked around downstream; proposed fix below

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

## Suggested fix

Scaffold a second `tsconfig.json` at the **app root**, and demote the client one
to extending it:

```jsonc
// tsconfig.json  (app root — the one an editor finds first)
{
  "compilerOptions": {
    // …same options…
    // Resolved relative to THIS file, so it must reach into client/.
    "paths": { "@app/client": ["./client/src/index.ts"] },
    "typeRoots": ["./client/node_modules/@types"]
  },
  "include": ["app/**/*.tsx", "app/**/*.ts", "client/src"]
}
```

```jsonc
// client/tsconfig.json
{ "extends": "../tsconfig.json", "include": ["src", "../app/**/*.tsx"] }
```

Verified downstream: with this pair, `tsc --noEmit` is clean from both the app
root and `client/`, and every page resolves a project. `typeRoots` matters
because React's types are installed under `client/node_modules`, not at the app
root.

Two details worth keeping in the template:

- The alias path differs between the two files (`./src/index.ts` vs
  `./client/src/index.ts`) because TypeScript resolves `paths` relative to the
  config that declares them. Extending alone does not fix the editor; the root
  config must restate them.
- `client/src/generated/index.ts` is written by `cargo build` and deleted by
  `npm run gen`. Between those two steps the alias resolves to a barrel that
  exports nothing, so the editor reports every hook as missing. Worth a line in
  the scaffold's `AGENTS.md`: **run `cargo nextrs client generate`, then
  `cargo build`, before trusting editor diagnostics.**

## Why not just document it

A comment in `client/tsconfig.json` would not be found by someone whose symptom
appears in `app/**/page.tsx`. The config has to exist where the editor looks.
