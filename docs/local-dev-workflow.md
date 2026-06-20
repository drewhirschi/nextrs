# Local Dev Workflow

This is the canonical app-level setup for `cargo dev` in nextrs projects.

The goal is to keep the dev helper separate from the app being served. If the
helper is a binary inside the main app package, Cargo may build the full app
once just to run the helper, then build it again when the helper starts the
real server with local-dev environment. A tiny `xtask` package avoids that.

## Commands

```bash
cargo dev       # watch, rebuild, restart, live reload
cargo dev-once  # one foreground server run, no watcher
```

These are Cargo aliases, not built-in Cargo subcommands.

## Workspace Shape

```text
my-app/
├── .cargo/config.toml
├── Cargo.toml
├── app/
├── build.rs
├── client/
├── src/main.rs
└── xtask/
    ├── Cargo.toml
    └── src/main.rs
```

Root `Cargo.toml`:

```toml
[workspace]
members = ["app", "xtask"]
resolver = "3"
```

Use whatever member name holds the served app. In this repo it is `site`; in a
standalone app it may be the root package or an `app` member.

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

## Helper Behavior

The `xtask` helper should do four things:

- Spawn the real app command, usually `cargo run -p <app-package>`.
- Set `NEXTRS_SKIP_BUNDLE=0` for the child so local TSX bundles regenerate even
  if deploy config sets `NEXTRS_SKIP_BUNDLE=1`.
- Watch app inputs: Rust sources, `app/`, `client/src`, package files,
  `build.rs`, templates, static assets, and the env file the server loads.
- Restart the child cleanly on changes. In debug, `tower-livereload` handles the
  browser reload after the server comes back.

The helper should not run two long-lived commands in parallel as the default
dev path. If an app needs extra generated assets, make the helper own that flow
so `cargo dev` remains the one stable command.

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
from this repo and adjust the served package name and watch paths.

Longer term, the roadmap app-builder command should generate this exact shape
so new projects get the same `cargo dev` behavior by default.
