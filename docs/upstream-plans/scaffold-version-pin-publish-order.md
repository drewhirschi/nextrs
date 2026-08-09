# `create-nextrs-app` emits a version that is not published yet

- **Reported-in:** onenote-extractor
- **Date:** 2026-06-20
- **Status:** reported

## Problem

`create-nextrs-app --here` generated a `Cargo.toml` depending on `nextrs = "0.3"`
while crates.io still had only `0.2.x`. The scaffold therefore produced a
project that could not resolve its own dependencies:

```
cargo dev   # fails: no matching package named `nextrs` version ^0.3
```

The generator pins the in-development version rather than the latest *published*
one, so any scaffold generated between a version bump and its publish is born
broken. The failure lands on a first-run user, at the first command they type,
with an error that reads like their environment is wrong.

The escape hatch — pointing the dependency at a local checkout — exists but is
not discoverable at the moment of failure.

## Local workaround

- Point app dependencies at the local `../nextrs/nextrs` checkout.
- Patch `nextrs-macros` to `../nextrs/nextrs-macros`, so the local framework
  crate resolves its macro dependency outside the nextrs workspace.

## Proposal

- Emit the latest **published** version — query the index at generation time, or
  bake the published version in at release time so the generator cannot outrun
  crates.io.
- Alternatively, gate the bump: the version the scaffold emits changes only when
  the publish succeeds.
- Either way, make `--nextrs-path ../nextrs/nextrs` discoverable in the
  generated next-steps output, so a contributor working against a local checkout
  is not left guessing.

## Validation

- A generator test asserting the emitted dependency resolves against the real
  index.
- Release-process check that publishing precedes (or is atomic with) the
  scaffold's version bump.
