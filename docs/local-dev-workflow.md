# Local Dev Workflow

This is the canonical app-level setup for `cargo dev` in nextrs projects.

The goal is to keep the dev helper separate from the app being served. If the
helper is a binary inside the main app package, Cargo may build the full app
once just to run the helper, then build it again when the helper starts the
real server with local-dev environment. A tiny `xtask` package avoids that.

## Commands

```bash
cargo dev       # watch, rebuild, restart, full-page browser reload
cargo dev-once  # one foreground server run, no watcher
```

These are Cargo aliases, not built-in Cargo subcommands.

The baseline dev experience is full-page live reload: save a watched file,
`cargo dev` restarts the server, and the browser reloads after the restarted
server responds. That is not React HMR/Fast Refresh; React state is not
preserved across the reload.

## Workspace Shape

`app/` is the Nextrs route tree. It is not a required Cargo package name.
Avoid naming the served Cargo package `app` unless you really want that
overload.

A standalone project can keep the served app as the root package and add only
`xtask` as a workspace member:

```text
my-app/
├── .cargo/config.toml
├── Cargo.toml                      # root package + workspace
├── app/                            # Nextrs route tree
├── build.rs
├── client/
├── src/main.rs
└── xtask/
    ├── Cargo.toml
    └── src/main.rs
```

Root `Cargo.toml`:

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2024"

[workspace]
members = ["xtask"]
resolver = "3"
```

In a larger repo, the served app can be a separate workspace member:

```toml
[workspace]
members = ["site", "xtask"]
resolver = "3"
```

Use whatever package name actually serves the app: the root package, `site`,
`hhh`, or another member. The `xtask` default command is the only place that
needs to know that name.

`.cargo/config.toml`:

```toml
[alias]
dev = "run -p xtask -- dev"
dev-once = "run -p xtask -- dev-once"
```

`xtask/Cargo.toml`:

```toml
[package]
name = "xtask"
version = "0.1.0"
edition = "2024"
publish = false
```

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

The `xtask` helper should do four things:

- Spawn the real app command: `cargo run` for a root-package app, or
  `cargo run -p <app-package>` for a member package.
- Set `NEXTRS_SKIP_BUNDLE=0` for the child so local TSX bundles regenerate even
  if deploy config sets `NEXTRS_SKIP_BUNDLE=1`.
- Watch app inputs: Rust sources, the Nextrs `app/` route tree, `client/src`,
  JS package and lock files, `build.rs`, templates, static assets, and the env
  file the server loads.
- Restart the child cleanly on changes.

The served app should add `tower-livereload` in debug builds. The watcher owns
process restart; the app owns browser reload injection. That split keeps the
helper generic and keeps production builds free of reload machinery.

`cargo dev` should be the one stable user command. Internally, the helper may
own more than one child process when that becomes necessary, for example a
future frontend HMR/bundler process. The convention is not "only one process";
the convention is "one user-facing command owns the whole dev loop."

## Dev Tiers

Keep these distinct:

1. Baseline: rebuild, restart, and full-page browser reload on backend, env,
   route, and frontend source changes.
2. Next.js-style parity: frontend HMR/Fast Refresh under the same `cargo dev`
   command. This should preserve compatible React component state and falls
   back to full reload when needed.

## Env Files

The server and watcher should agree on the env file:

- Default: `.env`.
- Override: `NEXTRS_ENV_FILE=/path/to/file`.

Server startup:

```rust
if let Ok(path) = std::env::var("NEXTRS_ENV_FILE") {
    dotenvy::from_path(path).ok();
} else {
    dotenvy::dotenv().ok();
}
```

Watcher input:

```rust
let env_file = std::env::var_os("NEXTRS_ENV_FILE")
    .map(PathBuf::from)
    .unwrap_or_else(|| PathBuf::from(".env"));
```

## Distribution

This is app scaffolding, not behavior the `nextrs` library can automatically
install into an existing project. Today, copy `.cargo/config.toml` and `xtask/`
from this repo and adjust the served package command and watch paths.

Longer term, the roadmap app-builder command should generate this exact shape
so new projects get the same `cargo dev` behavior by default.
