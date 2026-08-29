# Consolidate the published CLI packages after cargo-nextrs 0.3

- **Recorded:** 2026-08-29
- **Status:** ready for a follow-up implementation PR
- **Predecessor:** PR 41, squash-merged as `fc8112b`

## Release state

PR 41's cron, generated-provider-config, and unified-deploy work is merged.
The following versions were published successfully and verified visible on
crates.io:

| Package | Published version | Ongoing? |
|---|---:|---|
| `nextrs-macros` | 0.1.8 | yes |
| `nextrs` | 0.6.1 | yes |
| `create-nextrs-app` | 0.1.6 | **freeze after this compatibility release** |
| `cargo-nextrs-dev` | 0.1.1 | **freeze** |
| `cargo-nextrs` | 0.3.0 | yes |

`create-nextrs-app` already prints a deprecation warning directing users to
`nextrs new` or `cargo nextrs new`. Do not yank its existing releases: old
installation instructions should continue to resolve, and crates.io does not
need a destructive cleanup for a superseded binary.

The docs site has not yet been redeployed for this release. Use the explicit
prebuilt path rather than the unsupported Vercel Git integration:

```bash
scripts/deploy-prebuilt.sh site
```

## Why five packages exist today

`cargo-nextrs` currently depends on both compatibility packages as libraries:

```text
cargo-nextrs
├── create-nextrs-app  # implementation behind `nextrs new`
└── cargo-nextrs-dev   # implementation behind `nextrs dev`
```

Consequently, cargo-nextrs 0.3.0 could not be published until
create-nextrs-app 0.1.6 existed in the registry. Continuing this dependency
direction would require publishing nominally deprecated packages whenever
their implementation changes.

## Target: three ongoing packages

```text
nextrs-macros  # procedural macros
nextrs         # application runtime
cargo-nextrs   # new, dev, generate, deploy, and cron commands
```

`nextrs-macros` must remain a separate proc-macro crate. Keeping the runtime
and CLI separate also avoids pulling CLI-only dependencies such as file
watchers, `syn`, and HTTP deployment clients into every application. Three is
therefore the sensible minimum, even though combining everything into one
Cargo package is technically possible.

## Implementation plan

1. Move the scaffold library from `crates/create-nextrs-app/src/lib.rs` into
   `cargo-nextrs` (a private module is sufficient unless another consumer is
   known).
2. Change `nextrs new` and `cargo nextrs new` to call that in-package module.
3. Move the development runner from `cargo-nextrs-dev` into `cargo-nextrs` and
   keep the existing `nextrs dev`, `cargo nextrs dev`, and compatibility binary
   behavior.
4. Remove the `create-nextrs-app` and `cargo-nextrs-dev` dependencies from
   `crates/cargo-nextrs/Cargo.toml`.
5. Remove the two legacy packages from the workspace only after confirming no
   workspace scripts or CI jobs invoke them by package name. They may remain
   as source-only compatibility wrappers temporarily if that makes migration
   safer, but they must not remain dependencies of `cargo-nextrs`.
6. Update the repository tree, installation docs, and release checklist to
   name only the three ongoing packages.
7. Do not publish another create-nextrs-app or cargo-nextrs-dev version unless
   a concrete compatibility defect in their already-published binaries
   requires it.

## Required compatibility tests

- `nextrs new`, `cargo nextrs new`, and `cargo-nextrs new` produce identical
  files.
- Fresh and `--adopt` scaffolds retain their overwrite protections and local
  `--nextrs-path` behavior.
- `nextrs dev`, `cargo nextrs dev`, and the installed compatibility binary
  retain argument forwarding, binary inference, watching, and child cleanup.
- `cargo publish --dry-run -p cargo-nextrs` succeeds without either legacy
  package being present in the registry at a new version.
- Installing `create-nextrs-app 0.1.6` still prints its deprecation notice and
  works independently; no yank is introduced.

## Release-process follow-ups

- A nextrs 0.6.1 package verification warned that `num-bigint 0.4.7` in the
  lockfile is yanked. Verification and publication succeeded, but update the
  lockfile to a non-yanked compatible release in routine dependency
  maintenance.
- Preserve publish order for the remaining crates:
  `nextrs-macros` → `nextrs` → `cargo-nextrs`.
- The local `main` branch in the release workspace had an unpublished commit
  (`a584161`, legacy-client migration notes) and diverged from `origin/main`.
  It was deliberately left untouched. This handoff branch was created from
  `origin/main`; do not reset or discard the local-main commit during cleanup.

