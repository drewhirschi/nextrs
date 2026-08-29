+++
title = "nextrs.toml"
description = "The app's single config source for identity and deployment settings"
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

# [vercel.extra]                      # non-framework Vercel keys only
# trailingSlash = false
```

Cron schedules are colocated with their protected handlers as
`#[nextrs::cron(schedule = "...")]`; see [Cron Jobs](/docs/crons).

## What `nextrs generate` writes

- **`.nextrs/vercel.json`** — generated framework state containing the Rust
  function, the catch-all rewrite to it, immutable
  caching for `/dist`, git auto-builds off, your regions/commands, and any
  Vercel-provider [crons](/docs/crons). It is overwritten atomically on every
  generation and passed to Vercel with `--local-config`. The adjacent README
  marks it as generated. `[vercel.extra]` is an escape hatch for Vercel keys
  the table does not model, but cannot override framework-owned `$schema`,
  regions, commands, functions, headers, rewrites, git, or crons.
- **`.nextrs/cloudflare/`** — the Worker shim for cloudflare-provider crons
  (gitignored, regenerated on demand).

The `[vercel]` table is optional; omitting it uses framework defaults. A root
`vercel.json` is never read or mutated, so it cannot become a second source
of deployment or cron configuration.

`nextrs deploy` and `nextrs cron deploy` both run `generate` first, so the
provider files can't drift from the config.
