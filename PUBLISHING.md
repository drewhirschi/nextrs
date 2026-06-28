# Publishing nextrs to crates.io

Status (2026-06-27): all four crates are **publish-ready** and dry-run clean. The only
remaining step needs a crates.io **auth token**, which wasn't available in the prep
environment. Run `cargo login <token>` first, then publish **in this order** (deps first):

```bash
cargo login <YOUR_CRATES_IO_TOKEN>

cargo publish -p nextrs-macros     # 0.1.2  (dry-run ✓)
cargo publish -p nextrs            # 0.3.0  (verifies once macros 0.1.2 is live)
cargo publish -p cargo-nextrs-dev  # 0.1.0  (dry-run ✓)
cargo publish -p create-nextrs-app # 0.1.0  (dry-run ✓)
```

## What the prep changed
- Removed `publish = false` from `cargo-nextrs-dev` and `create-nextrs-app`.
- Bumped `nextrs-macros` `0.1.0 → 0.1.2` and pinned `nextrs`'s dep to `0.1.2`.

## Why macros 0.1.2 (not 0.1.0 or 0.1.1)
crates.io already has `nextrs-macros 0.1.1` (published from the now-deleted `spicy-pocket`
branch), but `main` was still at `0.1.0`. Re-publishing `0.1.0` is impossible (older than
what's live) and reusing `0.1.1` is risky (its source may differ from `main`). Publishing
`main`'s macros fresh as **0.1.2** keeps the crates.io line monotonic (0.1.1 → 0.1.2) and
guarantees the published macros are exactly the source `nextrs 0.3.0` compiles against.

## Notes
- This publishes the **current (seeds) API** (`QuerySeed` / `seed_key` / `props.rs`). The
  planned **prefetch rename** lands later as `nextrs 0.4.0` (with `#[deprecated]` aliases).
- `nextrs`'s dry-run can't fully verify offline because its `nextrs-macros = "0.1.2"` dep
  isn't on crates.io yet — that's normal; it verifies during the real publish after step 1.
- Once published, `cargo install create-nextrs-app` + `cargo install cargo-nextrs-dev` work,
  which unblocks the default `create-nextrs-app` flow (ISSUES.md CRA-1/2/3).
- Keep `create-nextrs-app/src/main.rs`'s `VERSION` const in lockstep with the released
  `nextrs` version (ideally a CI check) so the scaffold never pins an unpublished version.
