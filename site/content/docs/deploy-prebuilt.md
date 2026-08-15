+++
title = "Deploy: Build Locally, Ship Artifacts"
description = "Run the complete Vercel build locally, verify the Rust function exists, and upload prebuilt output"
section = "Guides"
order = 12
+++

Vercel cloud builds can spend minutes compiling Rust and longer waiting for an
account build slot. A prebuilt deploy runs the same configured Vercel build on
your machine, then uploads only `.vercel/output`.

New nextrs apps make this the default by setting
`"git": { "deploymentEnabled": false }` and generating
`scripts/deploy-prebuilt.sh`. A git push alone does not deploy such a project.

## One-time setup

```bash
npm install --global vercel
vercel login
cargo install cargo-zigbuild
pip install ziglang             # or install Zig another way
cd your-app
vercel link
```

`cargo-zigbuild` targets the older glibc available in the function runtime.
Without it or Zig, the community runtime can finish without producing a Rust
function. The generated script explicitly checks the output before upload.

## Deploy a scaffolded app

From the application root:

```bash
scripts/deploy-prebuilt.sh           # production
scripts/deploy-prebuilt.sh --preview # preview
```

The script performs the equivalent of:

```bash
vercel pull --yes --environment=production
vercel build --prod
vercel deploy --prebuilt --prod
```

For preview mode, it omits `--prod` from build and deploy.

`vercel build` runs the root `installCommand` and `buildCommand` from
`vercel.json`. For a generated nextrs app that means:

1. root `npm ci` links `.nextrs/client` and installs React/Orval/TypeScript;
2. the current Rust OpenAPI contract generates fetch and React Query clients;
3. release Cargo compilation builds the Vercel adapter and browser bundles;
4. TypeScript emits client JavaScript and declarations.

Generated assets do not need to be committed. A prebuilt deployment changes
where this complete build runs, not what the build contains.

## Monorepo projects

The Vercel project Root Directory and the directory where you run
`vercel build` must agree. If the project declares a root directory such as
`site`, link that project and run the repository's deployment wrapper as
documented by the repository. If the app itself is the Vercel root, run its
generated script inside the app.

Workspace Cargo builds also need their produced function to remain inside the
uploaded project. A deployment wrapper can set a project-local
`CARGO_TARGET_DIR` when the workspace default would point outside the upload
root.

## Required verification

Before `vercel deploy --prebuilt`, confirm that the build actually contains a
function:

```bash
find .vercel/output/functions -name '*.func' -type d
```

The generated script refuses to deploy when this finds nothing. This catches
the most dangerous failure mode: a successful-looking static output with no
Rust function.

## Other gotchas

- Exclude Cargo targets and unrelated `node_modules` from Vercel source
  uploads; large build trees can exceed file-count or file-size limits.
- Pin `framework: null` in project settings if Vercel misidentifies the app as
  Next.js.
- Keep tests and smoke checks in CI. A prebuilt CLI upload does not imply that
  your normal pull-request checks ran.
- Install application dependencies only at the project root. Never run npm
  installation inside `.nextrs/client`.

## Prebuilt versus cloud builds

| | Cloud build | Prebuilt build |
|---|---|---|
| Trigger | git integration | deployment script |
| Build location | Vercel infrastructure | your machine or CI runner |
| Build steps | root install + generation + Cargo + client build | the same steps via `vercel build` |
| Queue | subject to account build slots | none |
| Upload | source, then build | Build Output only |

To choose cloud builds, re-enable Vercel's git deployment setting. The
generated build is self-contained in either mode; do not revive the old
workflow of skipping frontend bundling and committing `public/dist`.
