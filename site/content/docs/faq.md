+++
title = "FAQ"
description = "Short answers about NextRS development and deployment behavior"
section = "Guides"
order = 99
+++

## Do Vercel's automatic Git and pull-request previews work?

Not currently. NextRS generates Vercel configuration at
`.nextrs/vercel.json`, keeps it out of Git, and passes it explicitly to the
Vercel CLI during `nextrs deploy`. Vercel's Git integration clones the
repository and looks for committed configuration before NextRS has generated
that file, so an automatic deployment can miss the Rust function, rewrites,
headers, build commands, and cron declarations.

Use the supported explicit preview command instead:

```bash
nextrs deploy --preview
```

It generates the provider configuration locally and uploads a prebuilt
preview. Preview deployments intentionally skip cron triggers.

If the project is connected to a Git repository in Vercel, disable automatic
deployments in the Vercel project settings. A future Git-integration design
may add a committed bootstrap file, but NextRS does not generate one today.
