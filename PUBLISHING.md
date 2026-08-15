# Publishing nextrs to crates.io

Status (2026-08-15): the framework, unified CLI, and compatibility tools are
published on crates.io.

| crate | workspace | crates.io status |
| --- | --- | --- |
| `nextrs` | **0.5.1** | **0.5.1** |
| `nextrs-macros` | **0.1.7** | **0.1.7** |
| `cargo-nextrs-dev` | **0.1.1** | **0.1.1** |
| `create-nextrs-app` | **0.1.3** | **0.1.3** |
| `cargo-nextrs` | **0.1.0** | **0.1.0** |

The intended user install is now exclusively:

```bash
cargo install cargo-nextrs
```

That package installs `cargo-nextrs`, `nextrs`, and the temporary
`cargo-nextrs-dev` compatibility launcher. The crates.io name `next-rs` belongs
to an unrelated project; do not document `cargo install next-rs`.

## Publishing a new release

Publish **in this order** (deps first) with a `cargo login` done once on the machine:

```bash
cargo publish -p nextrs-macros      # only when its version changed
cargo publish -p nextrs             # only when its version changed
cargo publish -p cargo-nextrs-dev   # publish its library target first
cargo publish -p create-nextrs-app  # publish its scaffold library next
cargo publish -p cargo-nextrs       # unified CLI always goes last
```

When either compatibility library changes, publish it before publishing a new
`cargo-nextrs` version, then update the corresponding dependency version in
`crates/cargo-nextrs/Cargo.toml`. Published versions are immutable.

- Bump `nextrs`'s pinned `nextrs-macros = { version = ... }` dep together with the
  macros version.
- Keep `create-nextrs-app/src/lib.rs`'s emitted `nextrs = "0.x"` in lockstep with the
  released `nextrs` version (currently `0.5.1`) so the scaffold never pins an
  unpublished version. The scaffold tests assert this value.
- Verify the package contents and install surface before publishing:

  ```bash
  cargo package -p cargo-nextrs
  cargo install --path crates/cargo-nextrs --force
  cargo nextrs --help
  nextrs --help
  ```

- A machine with the standalone `cargo-nextrs-dev` package installed will have
  a binary-name collision when installing the unified package. Migrate it with
  `cargo uninstall cargo-nextrs-dev` before `cargo install cargo-nextrs`.

## History / version quirks

- `nextrs-macros` jumped `0.1.0 → 0.1.2` at first publish: crates.io already had a
  `0.1.1` published from the deleted `spicy-pocket` branch, so `main` published fresh as
  `0.1.2` to keep the line monotonic and guarantee published source matches `main`.
- `nextrs 0.3.0` was never published — `main` had already moved to `0.3.1` (route
  params release, PR #24) when the first publish happened.
- `create-nextrs-app` and `cargo-nextrs-dev` remain published for existing
  installations, but their executables are deprecated compatibility wrappers.
  Do not yank them while applications still reference the old commands.
