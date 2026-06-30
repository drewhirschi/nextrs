# Local Dev Workflow

For consumer apps, the canonical `cargo dev` setup is the `cargo-nextrs-dev`
Cargo subcommand. You install it once and wire a `dev` alias to it; nothing
about the dev helper lives inside the served app package. This is exactly what
`create-nextrs-app` generates.

Because the runner is a separately installed binary, running `cargo dev` never
builds the full app just to start the helper. The runner builds the app once
with `cargo build --bin <crate>`, runs the produced binary directly, and
restarts it after relevant file changes.

(This repo's own `site/` app uses exactly this runner — `cd site && cargo dev`
expands to `nextrs-dev --bin site` — so the framework dogfoods the tool it ships.
Install it from the workspace with `cargo install --path crates/cargo-nextrs-dev`.)

## Commands

```bash
cargo dev   # build, run, watch, rebuild, restart, full-page browser reload
```

`cargo dev` is a Cargo alias, not a built-in Cargo subcommand. It expands to
`cargo nextrs-dev --bin <crate>`.

The baseline dev experience is full-page live reload: save a watched file,
`cargo dev` rebuilds and restarts the server, and the browser reloads after the
restarted server responds. That is not React HMR/Fast Refresh; React state is
not preserved across the reload.

## Workspace Shape

Install the runner once:

```bash
cargo install cargo-nextrs-dev
```

`app/` is the Nextrs route tree. It is not a required Cargo package name.
Avoid naming the served Cargo package `app` unless you really want that
overload.

A standalone project keeps the served app as the root package; no extra
workspace member is needed for dev:

```text
my-app/
├── .cargo/config.toml
├── Cargo.toml
├── app/                            # Nextrs route tree
├── build.rs
├── client/
└── src/main.rs
```

`.cargo/config.toml`:

```toml
[alias]
dev = "nextrs-dev --bin my-app"
```

`<crate>` is the package/binary that serves the app. Generated apps set
`default-run` and a single `[[bin]]` so the name is unambiguous:

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2024"
default-run = "my-app"

[[bin]]
name = "my-app"
path = "src/main.rs"
```

In a larger repo the served app can be a separate workspace member; point the
alias at its binary with `--bin <crate>`. The alias is the only place that needs
to know the package or binary name.

Served app `Cargo.toml`:

```toml
[dependencies]
tower-livereload = "0.9"
```

Served app `main.rs`:

```rust
let app = nextrs::router::build_router_with_public(generated_registry(), &public_dir);

#[cfg(debug_assertions)]
let app = app.layer(tower_livereload::LiveReloadLayer::new());
```

## Helper Behavior

`cargo-nextrs-dev` does four things:

- Build the app with `cargo build --bin <crate>` without interrupting an
  in-progress Cargo build, then run the produced binary directly.
- Watch app inputs: `src`, the Nextrs `app/` route tree, `client/src`, JS
  package and lock files, `client/tsconfig.json`, `build.rs`, `Cargo.toml`,
  `Cargo.lock`, `.cargo/config.toml`, and `public`.
- Respect `.gitignore`/`.ignore`, plus built-in ignores for `target/`,
  `node_modules/`, generated client code, and `public/dist/`.
- Restart the child cleanly on changes.

The served app should add `tower-livereload` in debug builds. The runner owns
rebuild and process restart; the app owns browser reload injection. That split
keeps the runner generic and keeps production builds free of reload machinery.

`cargo dev` should be the one stable user command. Internally, the helper may
own more than one child process when that becomes necessary, for example a
future frontend HMR/bundler process. The convention is not "only one process";
the convention is "one user-facing command owns the whole dev loop."

## Dev Tiers

Keep these distinct:

1. Baseline: rebuild, restart, and full-page browser reload on backend, route,
   and frontend source changes.
2. Next.js-style parity: frontend HMR/Fast Refresh under the same `cargo dev`
   command. This should preserve compatible React component state and falls
   back to full reload when needed.

## Env Files

Generated apps load `.env` at startup with `dotenvy::dotenv().ok()`.
`cargo-nextrs-dev` does not watch `.env` (it is ignored), so restart `cargo dev`
after changing it.

## Distribution

New projects get this exact shape from `create-nextrs-app`: it scaffolds the
`.cargo/config.toml` alias, the `default-run`/`[[bin]]` package, the
`tower-livereload` debug layer, and prints `cargo install cargo-nextrs-dev` so
`cargo dev` works out of the box.

This repo's `site/` app uses the same shape — `site/.cargo/config.toml` carries
the alias and `cd site && cargo dev` runs the watcher, identical to a scaffolded
app:

```toml
[alias]
dev = "nextrs-dev --bin site"
```

(Historically this repo drove dev through a bespoke `xtask` workspace member
because the old repo-root layout wasn't a standard single-app layout. The reorg
that made `site/` self-contained removed `xtask` — `cargo-nextrs-dev` watches it
directly now.)
