+++
title = "Tooling Dependencies"
description = "The external tools a nextrs project uses, and which workflows need each one"
section = "Guides"
order = 14
+++

nextrs itself is a Cargo dependency, but the surrounding workflows lean on a
few external tools. None are needed at runtime — they are all build/deploy
machinery on your workstation or CI.

| Tool | Needed for | Install |
| --- | --- | --- |
| **Rust** (1.85+) | everything — the app is a Cargo project | [rustup.rs](https://rustup.rs) |
| **Node.js + npm** | the generated TypeScript client, TSX bundling, dev loop | [nodejs.org](https://nodejs.org) (LTS) |
| **Vercel CLI** | deploys (`vercel deploy --prebuilt`), env management (`vercel env`) | `npm i -g vercel` |
| **cargo-zigbuild + zig** | [prebuilt deploys](/docs/deploy-prebuilt) — cross-compiling the Vercel function locally for `x86_64-unknown-linux-gnu` | `cargo install cargo-zigbuild` + [ziglang.org](https://ziglang.org/download/) |
| **wrangler** (optional) | [Cloudflare cron triggers](/docs/crons) on a workstation — `nextrs cron deploy` without `CLOUDFLARE_API_TOKEN`; with a token the CLI calls the API directly | `npm i -g wrangler` |

Day-to-day development needs only Rust and Node — `nextrs dev` covers the
loop. The rest come in when you deploy: the Vercel CLI plus cargo-zigbuild for
the prebuilt path, and wrangler only if you declare Cloudflare-provider crons
and prefer its login flow over a `CLOUDFLARE_API_TOKEN`.
