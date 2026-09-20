+++
title = "Custom Build Command"
description = "For apps linking native libraries (OpenCV, .so files, GLIBC version errors on Vercel): build in a container with [build] command and let nextrs deploy verify the packaged output"
section = "Guides"
order = 15
+++

`nextrs deploy` compiles your server on the machine you run it on, with
`cargo zigbuild` pinned to an old glibc so the binary runs on Vercel. That is
enough for pure-Rust apps.

It stops being enough when the app links **native system libraries** —
OpenCV, MediaPipe, anything installed as a `.so`. Those libraries were built
against the build machine's own glibc, so a binary linked on a modern
workstation can't start on Vercel's older runtime. The fix is to compile
somewhere with old enough libraries, usually a container.

`[build] command` lets you do that without giving up `nextrs deploy`.

## When you need this

You need it if the deployed function fails at startup with errors like:

```
/var/task/executable: /lib64/libc.so.6: version `GLIBC_2.38' not found
error while loading shared libraries: libopencv_core.so.412: cannot open shared object file
```

or if you are maintaining your own deploy script only because the server has
to be compiled in a container. You do **not** need it for ordinary Rust
dependencies, including ones that compile bundled C code from source.

## What it changes

```toml
# nextrs.toml
[build]
command = "scripts/build-in-container.sh"
```

Every step of `nextrs deploy` stays the same except one: instead of compiling
and packaging the server bundles itself, nextrs runs your command, then checks
what it produced.

| Step | Without `[build]` | With `[build] command` |
| --- | --- | --- |
| plan, generate, `vercel pull`, env files, frontend | nextrs | nextrs |
| compile + package server bundles | nextrs, on this machine | **your command** |
| verify the packaged output | — | nextrs |
| `vercel deploy --prebuilt`, cron triggers | nextrs | nextrs |

The command runs with `sh -c` from the app root and gets:

| Variable | Meaning |
| --- | --- |
| `NEXTRS_BUNDLE_OUTPUT` | Absolute path the finished output must be written to. Emptied before the command runs. |
| `NEXTRS_BUILD_TARGET` | `vercel` |
| `NEXTRS_SKIP_BUNDLE` | `1` — the frontend is already built on the host; your app's `build.rs` reuses it |

## The packaging rules

nextrs didn't run the build, so it trusts only what it can check. After the
command exits successfully, `$NEXTRS_BUNDLE_OUTPUT` must satisfy:

1. **`bundle-manifest.json` equals this app's bundle plan** (`nextrs bundles
   plan`). Proves the build ran against the same routes, features, and config.
2. **Every planned bundle has
   `functions/__nextrs_functions/<name>.func/executable`**, and it is an
   x86-64 Linux (ELF) binary with the executable bit set.
3. **Every path in a bundle's `assets` exists inside its `.func/`
   directory**, at the same relative path.

A broken rule aborts the deploy before anything is uploaded and names the
rule. nextrs then rewrites the files that derive purely from the plan and
`nextrs.toml` — each `.vc-config.json`, `config.json`, `routing.json`, the
manifest — so regions, routing, and Vercel crons can't drift from your config
whatever the command wrote. If the output has no `static/`, `public/` is
copied in.

The easy way to satisfy all three is to run nextrs itself wherever the build
happens:

```bash
nextrs bundles build --vercel --output "$NEXTRS_BUNDLE_OUTPUT"
```

A command that assembles the layout by hand is equally valid — nextrs only
judges the directory.

## Worked example: shared libraries in a container

An app whose `vision` bundle links OpenCV and ships private `.so` files:

```toml
# nextrs.toml
[build]
command = "scripts/build-in-container.sh"

[bundles.vision]
features = ["face-inference"]
assets   = ["resources/vision"]     # models and lib/*.so ride along here
```

```bash
#!/usr/bin/env bash
# scripts/build-in-container.sh
set -euo pipefail
app=$(pwd)

docker run --rm --user "$(id -u):$(id -g)" \
  -e NEXTRS_BUNDLE_OUTPUT -e NEXTRS_SKIP_BUNDLE \
  -e CARGO_TARGET_DIR=/build/target \
  -e RUSTFLAGS='-C link-arg=-Wl,-rpath,$ORIGIN/resources/vision/lib' \
  -v my-app-build:/build \
  -v "$app:$app" -w "$app" \
  my-app-build-image \
  nextrs bundles build --vercel --output "$NEXTRS_BUNDLE_OUTPUT"
```

Two details make the shared libraries work, and neither is nextrs-specific:

- **`assets` puts the `.so` files beside the executable.** Declared assets are
  copied into the bundle's `.func/` directory at the same relative path.
- **The rpath is relative to the executable** (`$ORIGIN/...`), so the dynamic
  loader finds them at runtime wherever Vercel unpacks the function.

Mount the app at the **same path** inside the container so
`NEXTRS_BUNDLE_OUTPUT` means the same thing on both sides.

`nextrs deploy` also fails early, before any build, when a declared asset is
missing from disk — the usual symptom of a gitignored private library absent
from a fresh clone.

## Iterating on a build command

Check an output against the rules without deploying:

```bash
NEXTRS_BUNDLE_OUTPUT="$PWD/.vercel/output" scripts/build-in-container.sh
nextrs bundles verify                      # or: --output <PATH>
```

```
nextrs: bundle vision has no executable at …/vision.func/executable
```

`[build] command` is used only by `nextrs deploy`. `nextrs bundles build`
always compiles in-process, which is what makes it safe to call from inside
the command.
