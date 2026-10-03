# Build artifact cache — push builds, not just code

- **Date:** 2026-09-24
- **Status:** proposed, not started
- **Related:** [ROADMAP.md](../ROADMAP.md) (AWS Lambda deploy target, durable
  execution on S3/R2); the `[build] command` work in a39f6e1

## Problem

Rust builds are the slowest part of every loop that ships code:

- **Deploys.** Vercel cloud builds are off fleet-wide (2026-07-14) because each
  one cost 6–10 minutes plus time in a shared queue. `nextrs deploy` now builds
  locally with cargo-zigbuild and uploads a prebuilt `.vercel/output`. That's
  fast, but only on a machine with a warm `target/`.
- **CI.** `.github/workflows/ci.yml` compiles the workspace, both apps, and the
  test binaries from a `Swatinem/rust-cache` cache. That cache lives in GitHub's
  cache store: scoped per branch, capped at 10GB per repo, and never shared
  with a developer machine. A PR's first run is effectively cold.
- **Duplicate work.** A developer has usually built the exact commit being
  pushed. CI and any future cloud deploy build it again from scratch.

The idea is to push build artifacts alongside the code, so whatever runs next
(CI, a deploy, a teammate) downloads the result instead of recompiling it.

## Proposed Direction

There are two independent layers. They have different trade-offs, so adopt
them separately.

### Layer 1: shared compile cache (sccache on R2)

[sccache](https://github.com/mozilla/sccache) wraps `rustc` and stores
compiled crates in a bucket. Laptops and CI point at the same bucket.
Whoever compiles a crate first uploads it, and everyone else downloads it.

```bash
export RUSTC_WRAPPER=sccache
export SCCACHE_BUCKET=nextrs-build-cache
export SCCACHE_ENDPOINT=https://<account-id>.r2.cloudflarestorage.com
export SCCACHE_REGION=auto
export SCCACHE_S3_KEY_PREFIX=sccache/
# plus AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY for an R2 API token
```

- **Wins:** it's off the shelf and works with any cargo command (tests, clippy,
  the app builds). R2 charges no egress. The heavy dependencies (tokio, axum,
  rolldown, oxc, syntect) get compiled roughly once per toolchain bump instead
  of once per cache miss per runner.
- **Limits:**
  - It doesn't cache linking or the final binary crates.
  - It turns off incremental compilation for the crates it wraps.
  - Cache keys include absolute paths. `/home/drew/work/nextrs` and
    `/home/runner/work/nextrs/nextrs` miss each other unless both build from
    the same path, for example a fixed checkout path in CI and a matching
    symlink or path on laptops.
  - Toolchain and target must match. Pin the toolchain with
    `rust-toolchain.toml`.
- **Result:** builds drop to roughly "compile the workspace crates, then link."
  The build still happens.

### Layer 2: output cache keyed by source (skip the build entirely)

This is Turborepo's remote cache, applied to nextrs deploy outputs. The unit
cached is the finished, verified `.vercel/output`, or later a Lambda zip.

**Cache key.** A hash of everything that determines the output bytes:

| Input | Source |
|---|---|
| Source tree | `git rev-parse HEAD^{tree}`, from the tree rather than the commit, so a rebase or no-op merge still hits |
| Toolchain | `rustc -vV`, cargo-zigbuild and zig versions |
| Target | triple + glibc pin (e.g. `x86_64-unknown-linux-gnu.2.28`) |
| nextrs CLI | `cargo-nextrs` version (it writes plan-derived metadata) |
| Build config | the `[build]` and `[vercel]` tables of `nextrs.toml`, `[build] command` if set |
| Build env | an allowlist: `RUSTFLAGS`, `NEXTRS_*`, `CARGO_PROFILE_*` |

Hashing the whole repo tree is deliberately coarse. It's correct without a
dependency graph, and in practice each app root builds from its own commit.
Narrowing the key to the app's crate closure (`cargo metadata`) is a later
optimization, not a requirement.

**Push.** After a successful local build:

1. Refuse if the tree is dirty. An artifact built from uncommitted changes
   has no honest key.
2. Run `bundles verify` on the output. Only verified outputs get cached.
3. Upload `artifacts/<app>/<key>/output.tar.zst` and then
   `artifacts/<app>/<key>/manifest.json`, both with `If-None-Match: *`. Keys
   are write-once, so pushes are idempotent and a racing second push can't
   overwrite the first. This is the same conditional-write primitive as the
   durable-execution design, and R2 supports it on PutObject.

`manifest.json` records the key inputs (unhashed, for debugging misses), the
commit SHA, the output's sha256, the builder (hostname / CI run URL), and a
timestamp. The manifest is written last, so its presence means the upload is
complete.

**Pull.** Anything that needs the output (CI, `nextrs deploy`, a teammate)
computes the key, fetches the manifest, downloads and checks the sha256,
unpacks into `.vercel/output`, and runs `bundles verify` again before use. On
a miss it builds normally and pushes.

**CLI surface (sketch):**

```bash
nextrs artifacts key            # print the key and its inputs (debug misses)
nextrs artifacts push           # upload the current verified output
nextrs artifacts pull           # fetch into .vercel/output; exit 2 on a miss
nextrs deploy                   # pull on hit, build on miss, push after build
```

```toml
# nextrs.toml
[artifacts]
store = "s3://nextrs-build-cache/artifacts"
endpoint = "https://<account-id>.r2.cloudflarestorage.com"
push = "auto"      # auto | never — auto pushes after every clean local build
```

A `pre-push` git hook (`nextrs artifacts push || true`) makes "push the code"
also push the build without any change to habits.

## Implementation Notes

- **Where it plugs in.** `crates/cargo-nextrs/src/deploy.rs` already treats
  `.vercel/output` as an opaque, verifiable directory. The `[build] command`
  path (a39f6e1) runs an external build and then enforces the packaging
  rules. A cache hit is one more way to produce that directory, and the same
  verifier gates it. No new trust in the output *format*.
- **Storage client.** Use the `object_store` crate (`PutMode::Create` for
  write-once). This is the same choice as durable execution, so both features
  share credentials, config, and the local-filesystem backend for tests.
- **Credentials.** Split tokens: developer machines and `main` CI get
  read-write. PR CI gets read-only, so a PR can use the cache but not poison
  it. Fork PRs get no secrets and just build.
- **Retention.** An R2 lifecycle rule deletes `artifacts/` objects older than
  ~30 days. sccache entries get the same treatment under their prefix.
- **Lambda.** The target triple is part of the key, so a Lambda zip for
  `aarch64` is just another artifact. Nothing Vercel-specific is baked in.

### Trust model (the real decision)

A cache hit means production runs a binary that CI didn't compile. For a solo
fleet that's the whole point, but the risk should be explicit:

- A compromised or misconfigured laptop ships straight to production, with no
  rebuild to catch it.
- Anyone with a read-write token can plant an artifact under a key that a
  later honest build would compute. Write-once helps here because the first
  writer wins, but it doesn't help if the attacker writes first.

Mitigations, cheapest first:

1. Manifests record the builder, so every deployed artifact is attributable.
2. Only CI on `main` gets write access to keys that production deploys will
   consume. Developer pushes land in a separate `artifacts-dev/` prefix used
   for previews and CI speedups.
3. Artifacts are signed (minisign / sigstore) and the signature is checked at
   pull.
4. Periodic rebuild-and-compare in CI. Rust builds aren't reproducible by
   default (paths, build IDs), so compare normalized outputs or treat a
   mismatch as a signal, not a failure.

Start with (1) and (2).

## Rollout

1. **sccache on R2 in CI.** Add the env to `ci.yml` alongside rust-cache,
   using a fixed checkout path. Measure the hit rate and wall-clock over a
   week of PRs.
2. **sccache on developer machines.** Document the env in
   `docs/local-dev-workflow.md`. Confirm local/CI hits with matching paths.
3. **`nextrs artifacts key/push/pull`** plus `nextrs deploy` integration,
   behind `[artifacts]` in `nextrs.toml`. Dogfood on `site/`, whose deploys
   already go through `scripts/deploy-prebuilt.sh`.
4. **CI consumes output artifacts.** This needs one open question answered
   first (below).
5. **Scaffold.** `nextrs new` emits a commented `[artifacts]` section and
   the pre-push hook as opt-in.

## Open Questions

- **CI tests a different build than it ships.** `ci.yml` smokes a local
  `cargo build` of each app, not the zigbuilt `.vercel/output` function. For
  CI to benefit from Layer 2, it has to smoke the deploy artifact itself, for
  example by running the function binary behind a local `vercel_runtime`
  shim. That would close a real gap on its own, since the smoke currently
  never exercises what ships.
- **Key granularity.** Whole-repo tree hash vs per-app crate closure. Coarse
  first. Measure how often unrelated commits cause misses in the monorepo.
- **Bucket choice.** R2 (no egress, S3 API, same account as the cron shim) is
  the default. Plain S3 needs to work too, for Lambda-hosted apps.
- **Relationship to Vercel's own build cache.** Irrelevant while cloud builds
  are off. Revisit if they come back on.

## Validation

- `nextrs artifacts key` is stable across two clean checkouts of the same
  commit on different machines, and changes when any key input changes (unit
  test per input).
- Push refuses a dirty tree and an output that fails `bundles verify`.
- A second push of the same key is a no-op and doesn't overwrite. Integration
  test against `object_store`'s local backend and against R2.
- `nextrs deploy` on a cache hit performs no cargo invocation and ships an
  output byte-identical to the pushed one (sha256 matches the manifest).
- A PR-scoped (read-only) token can pull but gets 403 on push.
- CI wall-clock before/after sccache, recorded in this doc.
