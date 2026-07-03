# Pinned rolldown =1.1.0 Fails on Newer Stable rustc

- **Reported-in:** react-todos Vercel deploy (self-sufficient build work)
- **Date:** 2026-07-03
- **Status:** open

## Problem

`nextrs`'s tsx bundler pins `rolldown = "=1.1.0"` (and friends).
`rolldown_resolver 1.1.0` fails to compile on newer stable rustc with
`error[E0310]: the parameter type Fs may not live long enough` — hit on
Vercel, whose build image tracks latest stable. Any tsx app without a
`rust-toolchain.toml` pin fails to build there (or on a dev machine with
current stable).

Mitigated short-term by pinning `channel = "1.96.0"` in site/, the example,
and (new) the scaffold output.

## Proposed Direction

Bump the rolldown pin to a version that compiles on current stable (1.1.4 was
available at time of writing), verify the bundler behavior end-to-end
(app-shell build, css/svg/use-server loaders, chunking), then drop the
toolchain pins from site/example/scaffold.

## Implementation Notes

- All rolldown crates are pinned `=1.1.0` in `crates/nextrs/Cargo.toml`; bump
  them in lockstep.
- Watch for API drift in `rolldown::{Bundler, BundlerOptions, InputItem}` and
  the plugin crates the bundler configures.
- The scaffold's `rust-toolchain.toml` comment says to remove it when this
  lands — do that in the same change.

## Validation

- Full test suite + the react-todos browser e2e on both 1.96.0 and current
  stable.
- A react-todos Vercel preview deploy built WITHOUT the toolchain pin.
