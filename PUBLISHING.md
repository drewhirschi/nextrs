# Publishing nextrs to crates.io

Status (2026-07-18): **all four crates are live on crates.io.**

| crate | published |
| --- | --- |
| `nextrs` | **0.4.1** (WaitUntil / Vercel waitUntil) |
| `nextrs-macros` | **0.1.4** |
| `cargo-nextrs-dev` | **0.1.0** |
| `create-nextrs-app` | **0.1.2** (emits `nextrs = "0.4"`) |

`cargo install create-nextrs-app` + `cargo install cargo-nextrs-dev` now work, which
unblocks the default scaffold flow (ISSUES.md CRA-1/2/3), and generated apps'
`nextrs = "0.3"` resolves.

## Publishing a new release

Publish **in this order** (deps first) with a `cargo login` done once on the machine:

```bash
cargo publish -p nextrs-macros
cargo publish -p nextrs            # verifies against the just-published macros
NEXTRS_SKIP_BUNDLE=1 cargo publish -p cargo-nextrs-dev
NEXTRS_SKIP_BUNDLE=1 cargo publish -p create-nextrs-app
```

- Bump `nextrs`'s pinned `nextrs-macros = { version = ... }` dep together with the
  macros version.
- Keep `create-nextrs-app/src/main.rs`'s emitted `nextrs = "0.x"` in lockstep with the
  released `nextrs` version (ideally a CI check) so the scaffold never pins an
  unpublished version.

## History / version quirks

- `nextrs-macros` jumped `0.1.0 → 0.1.2` at first publish: crates.io already had a
  `0.1.1` published from the deleted `spicy-pocket` branch, so `main` published fresh as
  `0.1.2` to keep the line monotonic and guarantee published source matches `main`.
- `nextrs 0.3.0` was never published — `main` had already moved to `0.3.1` (route
  params release, PR #24) when the first publish happened.
- The planned **prefetch rename** lands later as `nextrs 0.4.0` (with `#[deprecated]`
  aliases).
