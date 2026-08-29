+++
title = "nextrs.toml"
description = "The app's single config source: app identity, Vercel settings, and cron schedules — vercel.json is generated from it"
section = "Guides"
order = 12
+++

`nextrs.toml` at the app root is where a nextrs app is configured. It is
the one file you edit; `nextrs generate` turns it into the provider files
Vercel and Cloudflare actually read. Newly scaffolded apps ship with one.

```toml
[app]
name = "myapp"                        # names generated resources (myapp-cron)
url = "https://myapp.vercel.app"      # the deployed app, for cron triggers

[vercel]
regions = ["pdx1"]
# runtime = "vercel-rust@4.0.11"      # defaults, shown for reference
# install_command = "npm ci"
# build_command = "npm run client:prepare && cargo build --release --bin index && npm run client:build"
# git_deploys = false                 # prebuilt deploys; a push ships nothing

# [vercel.extra]                      # raw keys merged into vercel.json last
# trailingSlash = false

[[crons]]
path = "/api/cron/digest"
schedule = "0 6 * * *"
```

## What `nextrs generate` writes

- **`vercel.json`** — when a `[vercel]` table is present, the whole file is
  generated: the Rust function, the catch-all rewrite to it, immutable
  caching for `/dist`, git auto-builds off, your regions/commands, and any
  vercel-provider [crons](/docs/crons). Don't hand-edit it; it's overwritten
  on the next generate. `[vercel.extra]` is the escape hatch for anything
  the table doesn't model.
- **`.nextrs/cloudflare/`** — the Worker shim for cloudflare-provider crons
  (gitignored, regenerated on demand).

Apps that predate `nextrs.toml` can adopt it incrementally: with no
`[vercel]` table, `generate` only replaces the `crons` key in an existing
`vercel.json` and leaves everything else untouched. Add the table when
you're ready to hand the file over.

`nextrs deploy` and `nextrs cron deploy` both run `generate` first, so the
provider files can't drift from the config.
