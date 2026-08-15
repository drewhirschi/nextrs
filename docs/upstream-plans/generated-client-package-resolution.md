# Generated Client Package Resolution And Clean Scaffold Ownership

- **Reported-in:** react-todos
- **Date:** 2026-08-12
- **Status:** open

## Problem

The generated TypeScript client is only partially integrated with its consuming
application. Existing pages can appear to work through bundler aliases or a
root TypeScript path, while a newly created nested TypeScript or JavaScript file
cannot resolve the package or its declarations. Installing dependencies inside
the generated client does not repair the application's dependency graph.

A whole-directory `.nextrs/client` ignore rule makes this especially subtle:
`npm ci` can exit successfully and create a workspace symlink whose target does
not exist. TypeScript then reports TS2307 even though installation appeared to
succeed. Merely checking for a root `node_modules` directory also misses a
deleted, dangling, copied, or stale generated-client entry.

The visible `client/` directory also mixes framework-generated API code,
browser runtime helpers, package-manager state, external publishing examples,
and user-authored React components. That makes ownership unclear and prevents a
simple project model where application code lives in `app/`, `components/`, and
`src/`, while generated artifacts are framework-owned.

## Proposed Direction

- Generate a genuine workspace/local package under `.nextrs/client`.
- Link it from the application's root `package.json`; never rely on a bundler
  alias or user-authored `tsconfig.paths` for the public client package.
- Publish stable `.` and `./react-query` exports with JavaScript and declaration
  output for both surfaces.
- Keep browser runtime plumbing separate from generated API-client modes.
- Make the root package-manager install the only required install and have the
  nextrs CLI orchestrate initial generation, rebuilds, and declaration output.
- Ignore the complete generated client package and OpenAPI contract. Keep a
  small framework template outside the package so normal generation and
  `cargo dev` can recreate a missing workspace target automatically.
- Validate that the root package entry resolves to the actual hidden workspace
  directory. Repair a missing or stale link from the root, then verify both
  public JavaScript and declaration exports after generation.
- Anchor frontend source resolution and dependencies at the application root,
  not the hidden generated-client directory.
- Treat only convention filenames as routes so arbitrary colocated React code
  is valid; put shared React components in top-level `components/`.
- Move shared Rust router construction into `src/app.rs`, with thin local and
  Vercel process entry points.

## Implementation Notes

`BundleConfig::client_dir` currently controls generated storage, bundler cwd,
dependency resolution, browser runtime, and the built-in `@/*` alias. Split
those responsibilities before moving generated files. Keep only the package
template available in a fresh checkout, then materialize the ignored package
and write its compiled JavaScript and declarations during generation.

The CLI should expose the same implementation through `cargo nextrs` and
`nextrs`, retain `cargo dev` through a generated Cargo alias, and keep the old
scaffold/dev commands as deprecated wrappers.

## Validation

- Scaffold a fresh app, perform one root dependency install, and generate the
  client without installing anything under `.nextrs/client`.
- Prove a fresh scaffold creates a precise root `.gitignore`, while adopt mode
  preserves an existing user file. Reproduce and reject a dangling workspace
  link even when some unrelated `node_modules` state already exists.
- From newly created deeply nested `.tsx` and checked `.js` consumers, import
  both the root client and `./react-query`; use TypeScript resolution tracing to
  prove they resolve through the root workspace link without `paths`.
- Assert inferred response, error, path, query, request-body, query-data, and
  mutation-variable types, expected invalid-input errors, and that public types
  do not fall back to `any`.
- Verify top-level and colocated React components bundle successfully and do
  not create routes.
- Build and run the local application and compile the Vercel adapter from the
  same `src/app.rs` router.
- Exercise `cargo dev`, `cargo nextrs dev`, and `nextrs dev` against the fresh
  scaffold.
