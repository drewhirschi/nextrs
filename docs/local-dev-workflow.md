# Local Dev Workflow

Install the unified CLI once:

```bash
cargo install cargo-nextrs
```

The package provides the same command family through two launchers:

```bash
nextrs dev
cargo nextrs dev
```

Scaffolded applications also retain the short command:

```bash
cargo dev
```

`cargo dev` is a project-local Cargo alias, not a built-in Cargo command. New
projects map it to the unified CLI:

```toml
# .cargo/config.toml
[alias]
dev = "nextrs dev --bin my-app"
```

The explicit `--bin` keeps the alias deterministic. When invoking `nextrs dev`
or `cargo nextrs dev` directly, it can usually be omitted: the runner reads
Cargo metadata, prefers the current package's `default-run`, and otherwise uses
its sole runnable binary. Ambiguous projects receive an error asking for
`--bin <name>`.

## What the dev command does

The runner:

- Builds the selected application binary without interrupting an in-progress
  Cargo build, then runs the resulting executable directly.
- Watches the Rust application, `app/` convention tree, shared components,
  package metadata, build hook, TypeScript configuration, and public assets.
- Respects `.gitignore` and `.ignore`, and excludes build products,
  `node_modules`, generated client output, and `public/dist`.
- Stops the previous application process group cleanly and restarts after a
  successful rebuild.

With `tower-livereload` enabled in debug builds, the browser refreshes after
the restarted server responds. This is full-page live reload, not React Fast
Refresh: React state is not preserved across a rebuild.

## Normal project shape

```text
my-app/
├── .cargo/config.toml             # `cargo dev` alias
├── app/                           # route conventions + colocated modules
├── components/                    # shared React components
├── src/
│   ├── app.rs                     # shared Rust application/router
│   └── main.rs                    # local/container process entry
├── .nextrs/
│   └── client/                    # generated npm workspace; do not edit
├── api/index.rs                   # Vercel adapter
├── public/
├── build.rs
├── Cargo.toml
├── package.json
└── tsconfig.json
```

The generated Cargo package sets `default-run` and names its normal server
binary explicitly. `src/app.rs` is the package library root shared by the
normal server and deployment adapter:

```toml
[package]
name = "my-app"
default-run = "my-app"

[lib]
path = "src/app.rs"

[[bin]]
name = "my-app"
path = "src/main.rs"
```

The application owns browser reload injection in debug builds; the CLI owns
watching, rebuilding, and process lifecycle. That split keeps production
builds free of dev-runner machinery.

## Creation and client generation

One install owns the entire workflow, and either launcher form is valid:

```bash
nextrs new my-app
# equivalent: cargo nextrs new my-app

nextrs client generate
# equivalent: cargo nextrs client generate
```

A fresh `new` command writes the project, runs `npm install` at the application
root, and runs the root `client:generate` script. Use `--no-install` to write
files only; the command then prints the exact root-level bootstrap steps.

Never run `npm install` inside `.nextrs/client`. It is generated output linked
as a workspace package, while application and generator dependencies are
owned by the root `package.json` and root `node_modules`.

## Migrating legacy installations

The old executables remain functional compatibility wrappers, but they print
deprecation warnings:

- `create-nextrs-app` → use `nextrs new` or `cargo nextrs new`.
- `cargo nextrs-dev` → use `nextrs dev` or `cargo nextrs dev`.

If the standalone watcher was installed previously, remove it before
installing the unified package because both packages provide a
`cargo-nextrs-dev` executable:

```bash
cargo uninstall cargo-nextrs-dev
cargo install cargo-nextrs
```

Existing aliases keep working because the unified package temporarily ships
the compatibility launcher:

```toml
[alias]
dev = "nextrs-dev --bin my-app" # legacy; still works with a warning
```

Update them when convenient:

```toml
[alias]
dev = "nextrs dev --bin my-app"
```

The separately installed `create-nextrs-app` package may also be uninstalled;
new documentation only teaches `cargo install cargo-nextrs`.

## Environment files

Generated apps load `.env` at startup with `dotenvy::dotenv().ok()`. The dev
runner intentionally ignores `.env`, so restart `cargo dev` after changing it.

## Future dev tier

React HMR/Fast Refresh remains a separate enhancement. It should eventually
live behind the same three user-facing dev forms (`cargo dev`, `nextrs dev`,
and `cargo nextrs dev`) with full reload as the fallback.
