+++
title = "Deploy: Build Locally, Ship Artifacts"
description = "Prebuilt Vercel deploys — skip the cloud build queue entirely; your machine compiles, Vercel receives"
section = "Guides"
order = 12
+++

Vercel normally clones your repo and builds it on their infrastructure. For a
Rust app that means a cold cargo build in the cloud (~6 minutes even
optimized), and on plans with one concurrent build slot, **queue time can
dwarf build time** — we've measured a 6-minute build sitting 100 minutes in
queue behind another project. Prebuilt deploys skip all of it: you compile
locally, upload only the artifacts, and the deploy is live in seconds.

## One-time setup

```bash
npm i -g vercel               # and `vercel login`
cargo install cargo-zigbuild  # cross-compiles against Lambda's older glibc
pip install ziglang           # zig toolchain (or install zig any other way)
cd your-app && vercel link    # writes .vercel/project.json
```

Why `cargo-zigbuild`: your machine's glibc is newer than the Lambda runtime's.
A natively linked binary can reference glibc symbols the Lambda image doesn't
have; zig's linker targets the older glibc explicitly. **If cargo-zigbuild or
zig is missing, `vercel build` doesn't fail — the Rust function is silently
absent from the output.** Check `.vercel/output/functions/` before deploying
(the script below does).

## Every deploy

```bash
vercel pull --yes --environment=production  # project settings + env vars
vercel build --prod                         # local compile, incl. the function
vercel deploy --prebuilt --prod             # upload artifacts only
```

Or use the script this repo ships:

```bash
scripts/deploy-prebuilt.sh site                      # docs site, production
scripts/deploy-prebuilt.sh examples/react-todos --preview
```

`vercel build` runs your `vercel.json` `installCommand`/`buildCommand`
locally and invokes the `vercel-rust` runtime's build (which picks up
cargo-zigbuild automatically), producing `.vercel/output/` in the Build
Output API format. `deploy --prebuilt` uploads that directory verbatim —
typically ~12 MB instead of your whole source tree.

## Gotchas (each one field-tested the hard way)

- **`.vercelignore` must exclude `target/`** — or the upload tries to ship
  your build directory (22k files).
- **Per-file 100 MB limit** on uploaded artifacts. A debug binary or a
  node_modules binary (e.g. tailwindcss) can exceed it; release builds and
  `.vercelignore` keep you under.
- **Pin `framework: null` in project settings** if Vercel misdetects your
  repo as a Next.js app and tries to run `next build`.
- **Commit author email must match a GitHub account connected to the Vercel
  project** — otherwise deploys land in a reason-less `BLOCKED` state. (Git
  deploys only; prebuilt CLI deploys sidestep this too.)
- CI checks don't run on prebuilt deploys — keep your test/smoke gate in CI
  on the same commit, and deploy after it's green.

## When to prefer which

| | Git-push deploys | Prebuilt deploys |
|---|---|---|
| Effort | zero — push to main | one script invocation |
| Wall clock | minutes + queue | **seconds** |
| Build environment | reproducible, Vercel's | your machine (or your CI runner) |
| Preview URLs per PR | automatic | manual (`--preview`) |

Scaffolded apps ship with the prebuilt flow as the default: `create-nextrs-app`
generates `scripts/deploy-prebuilt.sh` and sets `"git": { "deploymentEnabled":
false }` in `vercel.json`, so pushes never trigger cloud builds. Re-enable git
deploys (delete that key) if you prefer the push-to-deploy baseline — both can
coexist on one project.
