# `PORT` and `.env` handling is inconsistent across scaffold, examples, and docs

- **Reported-in:** onenote-extractor
- **Date:** 2026-06-20
- **Status:** partially addressed locally; upstream consistency pass outstanding

## Problem

Two separate inconsistencies, both cheap to hit:

1. **Generated apps read process env only.** `PORT=3002` in a `.env` file is
   silently ignored, because nothing loads `.env`. Setting a port the obvious
   way appears to do nothing.
2. **Examples disagreed with the scaffold.** The generated app, and
   `site/src/main.rs`, read `PORT` with a `3000` fallback, but
   `examples/react-todos/src/main.rs` hardcoded `0.0.0.0:3000`. So the example a
   reader is most likely to copy was the one that did not follow the pattern.

## Changes already made in the local checkout

- `examples/react-todos` now reads `PORT` with a `3000` fallback.
- `create-nextrs-app` adds `dotenvy = "0.15"`, calls `dotenvy::dotenv().ok()` at
  startup, and generates `.env.example` containing `PORT=3000`.

## Proposal

Make it a single stated rule and hold everything to it: **load `.env` first,
then read `PORT`, falling back to `3000`** — in the scaffold, every example, the
site, and the docs.

Generated projects should ship an example env file, and should load `.env` in
local development before reading configuration. Worth deciding explicitly
whether `.env` loading is debug-only; loading it in release is a surprise for
deployed environments where the platform supplies real env vars.

## Validation

- A scaffold test asserting `.env.example` is emitted and that `dotenv()` runs
  before the port is read.
- A grep-level check across examples and site that no `main.rs` hardcodes a
  listen address, so the inconsistency cannot reappear.
