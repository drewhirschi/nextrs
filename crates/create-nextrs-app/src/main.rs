use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const VERSION: &str = "0.4";

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let options = parse_args(std::env::args().skip(1))?;
    let target = match options.target {
        Some(path) => path,
        None if options.here => PathBuf::from("."),
        None => prompt_project_path()?,
    };

    if options.adopt {
        adopt(&target, options.nextrs_path.as_deref())?;
    } else {
        scaffold(&target, options.nextrs_path.as_deref())?;
    }
    Ok(())
}

fn print_help() {
    println!(
        "create-nextrs-app\n\nUSAGE:\n    create-nextrs-app <path> [--nextrs-path <path>]\n    create-nextrs-app --here [--nextrs-path <path>]\n    create-nextrs-app --adopt [<path> | --here] [--nextrs-path <path>]\n\nCreates a React-first nextrs app with /, /api/ping, and /slow.\n\nWith --adopt, generates the nextrs skeleton into an EXISTING repo instead:\nminimal content (one page, no demo routes), existing files are never\noverwritten (skipped and reported), an existing src/main.rs gets a\nsrc/main.rs.example beside it, and an existing Cargo.toml is left alone\nwith the dependency lines to merge printed instead.\n\nOPTIONS:\n    --here                Create the app in the current directory\n    --adopt               Graft the skeleton into an existing directory; never overwrite\n    --nextrs-path <path>  Use a local nextrs checkout instead of the published nextrs"
    );
}

#[derive(Debug, Default)]
struct Options {
    target: Option<PathBuf>,
    nextrs_path: Option<PathBuf>,
    here: bool,
    adopt: bool,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> io::Result<Options> {
    let mut options = Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            "--here" => {
                options.here = true;
            }
            "--adopt" => {
                options.adopt = true;
            }
            "--nextrs-path" => {
                let Some(path) = args.next() else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "--nextrs-path requires a value",
                    ));
                };
                options.nextrs_path = Some(PathBuf::from(path));
            }
            _ if arg.starts_with('-') => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unknown option: {arg}"),
                ));
            }
            _ => {
                if options.target.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("unexpected argument: {arg}"),
                    ));
                }
                options.target = Some(PathBuf::from(arg));
            }
        }
    }
    if options.here && options.target.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--here cannot be combined with a target path",
        ));
    }
    Ok(options)
}

fn prompt_project_path() -> io::Result<PathBuf> {
    print!("Project path: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "project path is required",
        ));
    }
    Ok(PathBuf::from(trimmed))
}

fn scaffold(target: &Path, nextrs_path: Option<&Path>) -> io::Result<()> {
    if target.exists() && target.read_dir()?.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists and is not empty", target.display()),
        ));
    }

    let crate_name = crate_name_from_path(target);
    let client_alias = format!("@{crate_name}/client");

    let dep = DependencySource::new(nextrs_path);
    let files = template_files(&crate_name, &client_alias, &dep);
    for (rel, body) in files {
        let path = target.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, body)?;
        #[cfg(unix)]
        if path.extension().is_some_and(|e| e == "sh") {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        }
    }

    println!("Created {}", target.display());
    println!();
    println!("Next steps (run in order):");
    if !is_current_dir(target) {
        println!("  cd {}", display_cd_path(target));
    }
    println!("  {}   # required: installs the `cargo dev` runner", dep.dev_tool_install_command());
    println!("  cd client && npm install && npm run gen && cd ..   # generate the typed client");
    println!("  cargo dev   # build + run with live reload");
    println!();
    println!("Tip: if `cargo dev` errors with \"no such command: nextrs-dev\", run the install line above.");
    println!();
    println!("Routes:");
    println!("  /          React page");
    println!("  /slow      React page + Rust prefetch + loading.tsx");
    println!("  /api/ping  Rust API route");

    Ok(())
}

/// What `--adopt` did with one template file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AdoptStatus {
    Created,
    SkippedExists,
}

/// Decide where (and whether) one adopt-mode template lands. Never overwrites:
/// an existing file is skipped, except `src/main.rs`, which falls back to
/// `src/main.rs.example` so the nextrs entrypoint is still available to merge.
fn plan_adopt_file(target: &Path, rel: &str) -> (String, AdoptStatus) {
    let rel = if rel == "src/main.rs" && target.join(rel).exists() {
        "src/main.rs.example".to_string()
    } else {
        rel.to_string()
    };
    let status = if target.join(&rel).exists() {
        AdoptStatus::SkippedExists
    } else {
        AdoptStatus::Created
    };
    (rel, status)
}

fn adopt(target: &Path, nextrs_path: Option<&Path>) -> io::Result<()> {
    std::fs::create_dir_all(target)?;

    let crate_name = crate_name_from_path(target);
    let client_alias = format!("@{crate_name}/client");
    let dep = DependencySource::new(nextrs_path);
    let files = adopt_template_files(&crate_name, &client_alias, &dep);

    let mut report: Vec<(String, AdoptStatus)> = Vec::new();
    for (rel, body) in &files {
        let (write_rel, status) = plan_adopt_file(target, rel);
        if status == AdoptStatus::Created {
            let path = target.join(&write_rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, body)?;
            #[cfg(unix)]
            if path.extension().is_some_and(|e| e == "sh") {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
            }
        }
        report.push((write_rel, status));
    }

    print_adopt_report(target, &report, &crate_name, &dep);
    Ok(())
}

fn print_adopt_report(
    target: &Path,
    report: &[(String, AdoptStatus)],
    crate_name: &str,
    dep: &DependencySource,
) {
    println!("Adopted nextrs into {}", target.display());
    println!();
    println!("Per-file report:");
    for (rel, status) in report {
        match status {
            AdoptStatus::Created => println!("  created         {rel}"),
            AdoptStatus::SkippedExists => println!("  skipped-exists  {rel}"),
        }
    }

    let skipped = |name: &str| {
        report
            .iter()
            .any(|(rel, status)| rel == name && *status == AdoptStatus::SkippedExists)
    };
    let created = |name: &str| {
        report
            .iter()
            .any(|(rel, status)| rel == name && *status == AdoptStatus::Created)
    };

    println!();
    println!("Next steps:");

    if skipped("Cargo.toml") {
        println!();
        println!("  Your Cargo.toml was left untouched. Merge these sections by hand:");
        println!();
        println!("    [[bin]]");
        println!("    name = \"{crate_name}\"   # or your existing binary; set default-run to it");
        println!("    path = \"src/main.rs\"");
        println!();
        println!("    [[bin]]");
        println!("    name = \"index\"          # the Vercel function entry (api/index.rs)");
        println!("    path = \"api/index.rs\"");
        println!();
        println!("    [build-dependencies]");
        println!("    nextrs = {}", dep.build_dependency());
        println!();
        println!("    [dependencies]");
        println!("    nextrs = {}", dep.runtime_dependency());
        println!("    axum = \"0.8\"");
        println!("    dotenvy = \"0.15\"");
        println!("    tokio = {{ version = \"1\", features = [\"full\"] }}");
        println!("    tower = \"0.5\"");
        println!("    vercel_runtime = {{ version = \"2\", features = [\"axum\"] }}");
        println!("    http = \"1\"");
        println!("    serde = {{ version = \"1\", features = [\"derive\"] }}");
        println!("    tower-livereload = \"0.9\"");
        println!("    utoipa = \"5\"");
    }
    if created("src/main.rs.example") {
        println!();
        println!("  src/main.rs already exists — the nextrs entrypoint was written to");
        println!("  src/main.rs.example. Merge it into your main.rs: the two required");
        println!("  pieces are the include!(...nextrs_routes.rs) line and serving the");
        println!("  router from generated_registry().");
    } else if skipped("src/main.rs.example") {
        println!();
        println!("  Both src/main.rs and src/main.rs.example already exist — nothing was");
        println!("  written for the entrypoint. See a fresh `create-nextrs-app` app for");
        println!("  the reference main.rs.");
    }
    if skipped(".gitignore") {
        println!();
        println!("  .gitignore was left untouched — make sure it covers:");
        println!("  /target  /public/dist  /node_modules  /client/node_modules  .env");
    }

    println!();
    println!("  Then, in order:");
    println!("    cargo install cargo-nextrs-dev              # the `cargo dev` runner");
    println!("    cd client && npm install && cd ..           # bundler resolves imports from client/node_modules");
    println!("    cargo dev                                   # build + run with live reload");
    println!();
    println!("  Add API routes as app/**/route.rs with #[nextrs::api], then generate the");
    println!("  typed client: cd client && npm run gen");
    println!();
    println!("  Porting guide (strangler pattern, conventions, gotchas):");
    println!("    https://nextrs-docs.vercel.app/docs/porting");
}

/// The `--adopt` template set: the fresh-app wiring minus the demo content —
/// no /slow route, no /api/ping, no demo stylesheet; one minimal page that
/// imports nothing, so the app builds before the typed client is generated.
fn adopt_template_files(
    crate_name: &str,
    client_alias: &str,
    dep: &DependencySource,
) -> Vec<(&'static str, String)> {
    template_files(crate_name, client_alias, dep)
        .into_iter()
        .filter(|(rel, _)| {
            !matches!(
                *rel,
                "app/layout.tsx"
                    | "app/page.tsx"
                    | "app/slow/page.tsx"
                    | "app/slow/loading.tsx"
                    | "app/slow/prefetch.rs"
                    | "app/api/ping/route.rs"
                    | "public/style.css"
            )
        })
        .chain([("app/page.tsx", adopt_page_tsx())])
        .collect()
}

fn adopt_page_tsx() -> String {
    r#"export default function Page() {
  return (
    <main>
      <h1>nextrs is wired up.</h1>
      <p>
        Replace this page, then graft your app in: pages under{" "}
        <code>app/**/page.tsx</code>, API handlers in <code>app/**/route.rs</code>,
        auth in <code>middleware.rs</code>. See AGENTS.md and{" "}
        <a href="https://nextrs-docs.vercel.app/docs/porting">the porting guide</a>.
      </p>
    </main>
  );
}
"#
    .into()
}

fn is_current_dir(path: &Path) -> bool {
    path.as_os_str() == "." || path.as_os_str().is_empty()
}

fn display_cd_path(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        ".".to_string()
    } else {
        path.display().to_string()
    }
}

fn crate_name_from_path(path: &Path) -> String {
    let current_dir_name = || {
        std::env::current_dir()
            .ok()
            .and_then(|path| path.file_name().and_then(OsStr::to_str).map(str::to_string))
    };
    let raw = if is_current_dir(path) {
        current_dir_name()
    } else {
        path.file_name()
            .and_then(OsStr::to_str)
            .filter(|name| !name.trim().is_empty())
            .map(str::to_string)
    }
    .unwrap_or_else(|| "nextrs-app".to_string());

    let mut out = String::new();
    let mut last_was_sep = false;
    for ch in raw.chars().flat_map(char::to_lowercase) {
        let valid = ch.is_ascii_alphanumeric() || ch == '_' || ch == '-';
        let next = if valid { ch } else { '-' };
        if next == '-' || next == '_' {
            if last_was_sep {
                continue;
            }
            last_was_sep = true;
        } else {
            last_was_sep = false;
        }
        out.push(next);
    }
    let out = out.trim_matches(|ch| ch == '-' || ch == '_').to_string();
    if out.is_empty() {
        "nextrs-app".to_string()
    } else if out
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic())
    {
        out
    } else {
        format!("app-{out}")
    }
}

enum DependencySource {
    Version,
    Path(PathBuf),
}

impl DependencySource {
    fn new(path: Option<&Path>) -> Self {
        match path {
            Some(path) => {
                let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
                Self::Path(path)
            }
            None => Self::Version,
        }
    }

    fn build_dependency(&self) -> String {
        match self {
            Self::Version => format!(r#"{{ version = "{VERSION}", features = ["build", "tsx"] }}"#),
            Self::Path(path) => format!(
                r#"{{ path = "{}", features = ["build", "tsx"] }}"#,
                toml_string(&path.display().to_string())
            ),
        }
    }

    fn runtime_dependency(&self) -> String {
        match self {
            Self::Version => format!(r#"{{ version = "{VERSION}", features = ["vercel"] }}"#),
            Self::Path(path) => format!(
                r#"{{ path = "{}", features = ["vercel"] }}"#,
                toml_string(&path.display().to_string())
            ),
        }
    }

    fn dev_tool_install_command(&self) -> String {
        match self {
            Self::Version => "cargo install cargo-nextrs-dev".to_string(),
            Self::Path(path) => {
                let runner = path
                    .parent()
                    .map(|parent| parent.join("cargo-nextrs-dev"))
                    .unwrap_or_else(|| PathBuf::from("cargo-nextrs-dev"));
                format!(
                    "cargo install --path {} --force",
                    display_shell_path(&runner)
                )
            }
        }
    }
}

fn toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn display_shell_path(path: &Path) -> String {
    let value = path.display().to_string();
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-'))
    {
        value
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn template_files(
    crate_name: &str,
    client_alias: &str,
    dep: &DependencySource,
) -> Vec<(&'static str, String)> {
    vec![
        (".gitignore", gitignore()),
        (".env.example", env_example()),
        (".cargo/config.toml", cargo_config_toml(crate_name)),
        ("Cargo.toml", cargo_toml(crate_name, dep)),
        ("AGENTS.md", agents_md(crate_name, client_alias)),
        ("build.rs", build_rs(client_alias)),
        ("src/main.rs", main_rs()),
        ("src/bin/dump-openapi.rs", dump_openapi_rs()),
        ("api/index.rs", api_index_rs()),
        ("vercel.json", vercel_json()),
        ("scripts/deploy-prebuilt.sh", deploy_prebuilt_sh()),
        ("app/layout.tsx", layout_tsx()),
        ("app/page.tsx", page_tsx(client_alias)),
        ("app/slow/page.tsx", slow_page_tsx(client_alias)),
        ("app/slow/loading.tsx", slow_loading_tsx()),
        ("app/slow/prefetch.rs", slow_prefetch_rs()),
        ("app/api/ping/route.rs", ping_route_rs()),
        ("client/package.json", client_package_json(crate_name)),
        ("client/orval.config.ts", client_orval_config_ts()),
        ("client/tsconfig.json", client_tsconfig_json(client_alias)),
        ("client/src/index.ts", client_index_ts()),
        ("client/src/nextrs-client.ts", nextrs_client_ts()),
        ("rust-toolchain.toml", rust_toolchain_toml()),
        ("public/style.css", style_css()),
    ]
}

fn agents_md(crate_name: &str, client_alias: &str) -> String {
    format!(
        r#"# {crate_name} — contract for coding agents

This is a [nextrs](https://nextrs-docs.vercel.app/docs/getting-started) app:
Rust (Axum) serving Next.js-style file routes with React `.tsx` pages. The
scaffold generated the wiring below — treat it as framework, not app code.

## The app/ tree is the router

Every directory under `app/` is a URL segment. The build step discovers these
files and wires the router — never register routes by hand:

| File | Role |
|---|---|
| `page.{{tsx,rs,html}}` | The content for this URL (`.tsx` = client-rendered React) |
| `layout.tsx` or `layout.rs` + `layout.html` | Wraps this segment's children (Askama layouts need `{{{{ children|safe }}}}`) |
| `loading.{{tsx,rs,html}}` | Skeleton streamed while the page computes |
| `middleware.rs` | Guard, runs before anything renders |
| `route.rs` | API handlers — one `pub async fn get/post/...` per method, `#[nextrs::api]` for the typed client |
| `prefetch.rs` | Server data seeding a `page.tsx`'s React Query cache (requires the `.tsx` sibling) |

A `.tsx` slot is exclusive: it cannot coexist with `.rs`/`.html` of the same
name. Full reference: <https://nextrs-docs.vercel.app/docs/conventions>

## Never hand-roll what the scaffold generates

`build.rs`, `src/main.rs`, `api/index.rs`, `vercel.json`,
`scripts/deploy-prebuilt.sh`, `rust-toolchain.toml`, and the `client/`
package are generated wiring. Extend them if you must; do not replace them
with improvised versions. Never edit generated output: `client/src/generated/**`,
`client/openapi.json`, and `public/dist/` are rebuilt on every build. The
seams for app code are `app/**`, `client/src/index.ts`, and
`client/package.json`.

## The client package and the bare-import rule

`client/` is a real npm package; pages import it as `{client_alias}`.

- **Every bare import used by any `.tsx` file must be installed in
  `client/package.json`** — the bundler resolves from `client/node_modules`
  and errors on unresolved bare imports. Adding a dependency means adding it
  there and running `npm install` in `client/`.
- **Never hand-write API types.** After changing `#[nextrs::api]` routes, run
  `npm run gen` in `client/` to regenerate the typed hooks from OpenAPI.
  Guide: <https://nextrs-docs.vercel.app/docs/typesafe-client>

## Dev loop

```bash
cargo dev   # build + run + watch (alias for nextrs-dev; `cargo install cargo-nextrs-dev` once)
```

Don't substitute a hand-rolled watch script — the runner knows which inputs
(Rust, templates, `app/`, `public/`, env files) require a restart.

## Diagnosing a slow route

Every response carries a `Server-Timing` breakdown — read it before adding
any logging:

```bash
curl -sI http://localhost:3000/todos | grep -i server-timing
# server-timing: mw;dur=1.2, seed;dur=430.0, handler;dur=445.1, total;dur=447.0, route;desc="/todos"
```

`mw` = middleware chain, `seed` = `prefetch.rs` data seeding, `handler` =
page render or API fn. When `handler` is the mystery, extract
`nextrs::Timing` and wrap the suspects — the segment appears in the same
header on the next request:

```rust
pub async fn get(timing: nextrs::Timing, Extension(db): Extension<Db>) -> Json<Vec<Todo>> {{
    let todos = timing.span("db", db.list()).await;
    Json(todos)
}}
```

The same data fires as `tracing` events (`RUST_LOG=nextrs=info` locally;
Vercel function logs in production). Full guide, including OpenTelemetry
export: <https://nextrs-docs.vercel.app/docs/telemetry>

## Deploys are prebuilt

Git auto-builds are OFF (`vercel.json` sets `git.deploymentEnabled: false`);
pushing deploys nothing. The deploy path is:

```bash
scripts/deploy-prebuilt.sh             # production
scripts/deploy-prebuilt.sh --preview   # preview
```

Guide: <https://nextrs-docs.vercel.app/docs/deploy-prebuilt>

## Porting into this app

Bringing routes over from an existing app? Graft them into this skeleton —
`route.ts` bodies become `route.rs` handlers, auth becomes `middleware.rs`,
React pages drop into `app/**/page.tsx` — rather than assembling parallel
structure around it. The paved road, including the strangler pattern for
incremental conversion and the gotchas list:
<https://nextrs-docs.vercel.app/docs/porting>
"#
    )
}

fn rust_toolchain_toml() -> String {
    r#"# Vercel's Rust runtime defaults to an rustc BELOW the tsx bundler's MSRV
# (observed: 1.92.0 vs oxc's required 1.94.0), so an unpinned deploy fails at
# `cargo build` with "rustc 1.92.0 is not supported". The pin is a floor, not
# a coupling: rustup honors it everywhere, and RUSTUP_TOOLCHAIN overrides it
# per-environment. Keep in sync with nextrs's rolldown/oxc MSRV.
[toolchain]
channel = "1.96.0"
"#
    .into()
}

fn gitignore() -> String {
    "/target\n/public/dist\n/node_modules\n/client/node_modules\n.env\n".into()
}

fn env_example() -> String {
    "PORT=3000\n".into()
}

fn cargo_config_toml(crate_name: &str) -> String {
    format!(
        r#"[alias]
dev = "nextrs-dev --bin {crate_name}"
"#
    )
}

fn cargo_toml(crate_name: &str, dep: &DependencySource) -> String {
    let build_dependency = dep.build_dependency();
    let runtime_dependency = dep.runtime_dependency();
    format!(
        r#"[package]
name = "{crate_name}"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
publish = false
default-run = "{crate_name}"

[[bin]]
name = "{crate_name}"
path = "src/main.rs"

[[bin]]
name = "index"
path = "api/index.rs"

[build-dependencies]
nextrs = {build_dependency}

[dependencies]
nextrs = {runtime_dependency}
axum = "0.8"
dotenvy = "0.15"
tokio = {{ version = "1", features = ["full"] }}
tower = "0.5"
vercel_runtime = {{ version = "2", features = ["axum"] }}
http = "1"
serde = {{ version = "1", features = ["derive"] }}
tower-livereload = "0.9"
utoipa = "5"
"#
    )
}

fn build_rs(client_alias: &str) -> String {
    format!(
        r#"fn main() {{
    nextrs::build::emit_registry("app", "src/main.rs", "nextrs_routes.rs")
        .expect("nextrs::build::emit_registry failed");

    nextrs::bundle::bundle_pages(&nextrs::bundle::BundleConfig {{
        app_dir: "app",
        client_dir: "client",
        client_alias: "{client_alias}",
        public_dist: "public/dist",
        ..Default::default()
    }})
    .expect("nextrs::bundle::bundle_pages failed");
}}
"#
    )
}

fn main_rs() -> String {
    r#"include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let public_dir = std::env::var("NEXTRS_PUBLIC_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/public").to_string());

    let app = nextrs::router::build_router_with_public(generated_registry(), &public_dir)
        .merge(nextrs::openapi::spec_router(generated_openapi()));

    #[cfg(debug_assertions)]
    let app = app.layer(tower_livereload::LiveReloadLayer::new());

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let listener = bind_with_fallback(port).await;
    let local = listener.local_addr().expect("listener has a local addr");
    println!("listening on http://{local}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

/// Bind `0.0.0.0:start`, or the next free port up to `start + 20` if it's taken.
async fn bind_with_fallback(start: u16) -> tokio::net::TcpListener {
    for port in start..start.saturating_add(20) {
        match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
            Ok(listener) => {
                if port != start {
                    eprintln!("Port {start} is in use; bound {port} instead (set PORT to choose).");
                }
                return listener;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(e) => {
                eprintln!("Failed to bind 0.0.0.0:{port}: {e}");
                std::process::exit(1);
            }
        }
    }
    eprintln!("No free port in {start}..{}. Stop the process using it, or set PORT.", start.saturating_add(20));
    std::process::exit(1);
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
"#
    .into()
}

fn dump_openapi_rs() -> String {
    r#"include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

fn main() {
    let spec = generated_openapi();
    let json = spec.to_pretty_json().expect("serialize OpenAPI document");
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/client/openapi.json");
    std::fs::write(out, json).expect("write client/openapi.json");
    eprintln!("wrote {out}");
}
"#
    .into()
}

fn api_index_rs() -> String {
    r#"use nextrs::vercel::StreamingVercelLayer;
use tower::ServiceBuilder;

include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    let router = nextrs::router::build_router(generated_registry())
        .merge(nextrs::openapi::spec_router(generated_openapi()));
    let app = ServiceBuilder::new()
        .layer(StreamingVercelLayer::new())
        .service(router);

    vercel_runtime::run(app).await
}
"#
    .into()
}

fn deploy_prebuilt_sh() -> String {
    r#"#!/bin/bash
# Prebuilt Vercel deploy: build on YOUR machine, upload only artifacts.
# Cloud builds recompile the whole Rust dependency tree from scratch on a
# small builder (~6-10 minutes, plus per-account queue time); this flow
# deploys in seconds. Git-push auto-builds are disabled in vercel.json
# ("git": {"deploymentEnabled": false}) — this script IS the deploy path.
#
#   scripts/deploy-prebuilt.sh             # production
#   scripts/deploy-prebuilt.sh --preview   # preview deploy
#
# One-time setup:
#   npm i -g vercel && vercel login && vercel link
#   cargo install cargo-zigbuild     # cross-compiles for Lambda's glibc
#   pip install ziglang              # zig toolchain (or install zig any way)
#
# Full guide: https://nextrs-docs.vercel.app/docs/deploy-prebuilt
set -euo pipefail
cd "$(dirname "$0")/.."

[ "${1:-}" = "--preview" ] && FLAGS=() || FLAGS=(--prod)

vercel pull --yes --environment=production > /dev/null
vercel build "${FLAGS[@]}"

# Refuse to ship if the Rust function silently failed to build (the classic
# missing-cargo-zigbuild failure: everything green, no binary in the output).
if ! find .vercel/output/functions -name '*.func' -type d 2>/dev/null | grep -q .; then
  echo "ERROR: no function in .vercel/output — is cargo-zigbuild installed and zig reachable?" >&2
  exit 1
fi

vercel deploy --prebuilt "${FLAGS[@]}"
"#
    .to_string()
}

fn vercel_json() -> String {
    r#"{
  "$schema": "https://openapi.vercel.sh/vercel.json",
  "installCommand": "cd client && npm ci",
  "buildCommand": "cd client && npx orval --config ./orval.config.ts && cd .. && cargo build --release --bin index",
  "functions": {
    "api/index.rs": {
      "runtime": "vercel-rust@4.0.11"
    }
  },
  "git": { "deploymentEnabled": false },
  "headers": [
    {
      "source": "/dist/(.*)",
      "headers": [
        {
          "key": "Cache-Control",
          "value": "public, max-age=31536000, immutable"
        }
      ]
    }
  ],
  "rewrites": [
    {
      "source": "/(.*)",
      "destination": "/api/index"
    }
  ]
}
"#
    .into()
}

fn layout_tsx() -> String {
    r#"import type { ReactNode } from "react";

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <div className="app-shell">
      <header className="topbar">
        <a href="/" className="brand">nextrs</a>
        <nav>
          <a href="/">Home</a>
          <a href="/slow">Slow prefetch</a>
        </nav>
      </header>
      {children}
    </div>
  );
}
"#
    .into()
}

fn page_tsx(client_alias: &str) -> String {
    format!(
        r#"import {{ useGetApiPing }} from "{client_alias}";

export default function Page() {{
  const ping = useGetApiPing({{ query: {{ enabled: false }} }});

  return (
    <main className="page">
      <section className="panel">
        <p className="eyebrow">React route</p>
        <h1>Build React apps with Rust routes.</h1>
        <p>
          This page renders immediately in the browser. The button calls a Rust
          route handler at <code>/api/ping</code> through a generated typed client.
        </p>
        <button type="button" onClick={{() => ping.refetch()}} disabled={{ping.isFetching}}>
          {{ping.isFetching ? "Pinging..." : "Ping Rust"}}
        </button>
        <p className="result">{{ping.data?.data.message ?? "Not called yet"}}</p>
      </section>
    </main>
  );
}}
"#
    )
}

fn slow_page_tsx(client_alias: &str) -> String {
    format!(
        r#"import {{ useSeed }} from "{client_alias}";

type SlowData = {{
  message: string;
}};

export default function SlowPage() {{
  const data = useSeed<SlowData>(["/slow/message"]);

  return (
    <main className="page">
      <section className="panel">
        <p className="eyebrow">Server prefetch</p>
        <h1>Loaded after Rust finished.</h1>
        <p>{{data?.message ?? "No server seed found."}}</p>
      </section>
    </main>
  );
}}
"#
    )
}

fn slow_loading_tsx() -> String {
    r#"export default function Loading() {
  return (
    <main className="page">
      <section className="panel loading-panel">
        <p className="eyebrow">Server prefetch</p>
        <h1>Loading from Rust...</h1>
        <p>This route waits two seconds in <code>prefetch.rs</code>.</p>
      </section>
    </main>
  );
}
"#
    .into()
}

fn slow_prefetch_rs() -> String {
    r#"use std::time::Duration;

pub async fn prefetch(_req: http::Request<axum::body::Body>) -> nextrs::QuerySeed {
    tokio::time::sleep(Duration::from_secs(2)).await;

    nextrs::QuerySeed::new()
        .seed(async {
            nextrs::SeedEntry {
                key: nextrs::seed_key("/slow/message", None),
                data: nextrs::serde_json::json!({
                    "message": "Loaded from Rust after a two second delay.",
                }),
            }
        })
        .await
}
"#
    .into()
}

fn ping_route_rs() -> String {
    r#"use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema)]
pub struct PingResponse {
    pub message: String,
}

#[nextrs::api(
    get,
    responses((status = 200, description = "Pong", body = PingResponse)),
)]
pub async fn get() -> Json<PingResponse> {
    Json(PingResponse {
        message: "pong from Rust".to_string(),
    })
}
"#
    .into()
}

fn client_package_json(crate_name: &str) -> String {
    format!(
        r#"{{
  "name": "@{crate_name}/client",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {{
    "postinstall": "ln -sfn client/node_modules ../node_modules",
    "dump": "NEXTRS_SKIP_BUNDLE=1 cargo run --bin dump-openapi",
    "orval": "orval --config ./orval.config.ts",
    "gen": "npm run dump && npm run orval",
    "typecheck": "tsc --noEmit"
  }},
  "dependencies": {{
    "@tanstack/react-query": "^5.62.0",
    "@tanstack/react-router": "^1.87.0",
    "react": "^19.0.0",
    "react-dom": "^19.0.0"
  }},
  "devDependencies": {{
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "orval": "^7.3.0",
    "typescript": "^5.7.0"
  }}
}}
"#
    )
}

fn client_orval_config_ts() -> String {
    r#"import { defineConfig } from "orval";

export default defineConfig({
  api: {
    input: "./openapi.json",
    output: {
      mode: "tags-split",
      target: "./src/generated",
      schemas: "./src/generated/model",
      client: "react-query",
      httpClient: "fetch",
      baseUrl: "/",
      clean: true,
      prettier: false,
    },
  },
});
"#
    .into()
}

fn client_tsconfig_json(client_alias: &str) -> String {
    format!(
        r#"{{
  "compilerOptions": {{
    "target": "ES2020",
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true,
    "esModuleInterop": true,
    "forceConsistentCasingInFileNames": true,
    "paths": {{
      "{client_alias}": ["./src/index.ts"]
    }}
  }},
  "include": ["src", "../app/**/*.tsx"]
}}
"#
    )
}

fn client_index_ts() -> String {
    r#"import { useQueryClient } from "@tanstack/react-query";
import { useParams as useRouterParams } from "@tanstack/react-router";

export function useSeed<T>(key: unknown[]): T | undefined {
  return useQueryClient().getQueryData<{ data: T }>(key)?.data;
}

// Matched route params ([seg] segments). Pages get them as a `params` prop;
// deep components can call this. Backed by the app shell's TanStack Router so
// the values stay LIVE across soft navigation — the server's __nx_params__
// tag is only the boot-time snapshot and goes stale after a client-side nav.
export function useParams<T extends Record<string, string> = Record<string, string>>(): T {
  return useRouterParams({ strict: false }) as T;
}

// Everything orval generates — React Query hooks for components, plus plain
// typed clients (getX/updateX functions and URL builders) for event handlers,
// scripts, and tests. The framework regenerates ./generated/index.ts on every
// build, so new endpoints show up here without editing this file.
export * from "./generated";
"#
    .into()
}

fn nextrs_client_ts() -> String {
    r#"import type { QueryClient } from "@tanstack/react-query";

interface SeedEntry {
  key: unknown[];
  data: unknown;
}

export function readSeeds(): SeedEntry[] {
  const tag = document.getElementById("__nx_seeds__");
  if (!tag?.textContent) return [];
  try {
    return JSON.parse(tag.textContent) as SeedEntry[];
  } catch {
    return [];
  }
}

export function seedQueryClient(qc: QueryClient): void {
  for (const entry of readSeeds()) {
    qc.setQueryData(entry.key, {
      data: entry.data,
      status: 200,
      headers: new Headers(),
    });
  }
}
"#
    .into()
}

fn style_css() -> String {
    r#":root {
  color: #101418;
  background: #f7f8fb;
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

body {
  margin: 0;
}

a {
  color: inherit;
}

button {
  border: 1px solid #101418;
  background: #101418;
  color: white;
  border-radius: 6px;
  padding: 0.65rem 0.9rem;
  cursor: pointer;
}

code {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace;
}

.app-shell {
  min-height: 100vh;
}

.topbar {
  height: 64px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 32px;
  border-bottom: 1px solid #dde2e8;
  background: white;
}

.brand {
  font-weight: 700;
  text-decoration: none;
}

.topbar nav {
  display: flex;
  gap: 18px;
}

.topbar nav a {
  text-decoration: none;
  color: #4c5967;
}

.page {
  width: min(820px, calc(100vw - 32px));
  margin: 72px auto;
}

.panel {
  border: 1px solid #dde2e8;
  background: white;
  border-radius: 8px;
  padding: 32px;
}

.loading-panel {
  animation: pulse 1.2s ease-in-out infinite alternate;
}

.eyebrow {
  margin: 0 0 12px;
  color: #5d6c7b;
  font-size: 0.8rem;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.08em;
}

h1 {
  margin: 0 0 14px;
  font-size: 2rem;
  line-height: 1.1;
}

.result {
  margin-top: 18px;
  color: #2e3a46;
}

@keyframes pulse {
  from { opacity: 0.62; }
  to { opacity: 1; }
}
"#
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_names_are_sanitized() {
        assert_eq!(crate_name_from_path(Path::new("My App")), "my-app");
        assert_eq!(crate_name_from_path(Path::new("123")), "app-123");
        assert_eq!(
            crate_name_from_path(Path::new("hello_world")),
            "hello_world"
        );
    }

    #[test]
    fn here_flag_targets_current_directory() {
        let opts = parse_args(["--here".to_string()]).unwrap();
        assert!(opts.here);
        assert!(opts.target.is_none());
    }

    #[test]
    fn here_flag_rejects_target_path() {
        let err = parse_args(["--here".to_string(), "demo".to_string()]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("--here cannot be combined"));
    }

    #[test]
    fn templates_use_react_first_conventions() {
        let files = template_files("demo", "@demo/client", &DependencySource::Version);
        let names: Vec<_> = files.iter().map(|(name, _)| *name).collect();
        assert!(names.contains(&".cargo/config.toml"));
        assert!(names.contains(&"src/bin/dump-openapi.rs"));
        assert!(names.contains(&"api/index.rs"));
        assert!(names.contains(&"vercel.json"));
        assert!(names.contains(&"app/layout.tsx"));
        assert!(names.contains(&"app/page.tsx"));
        assert!(names.contains(&"app/slow/loading.tsx"));
        assert!(names.contains(&"app/slow/prefetch.rs"));
        assert!(names.contains(&"app/api/ping/route.rs"));
        assert!(names.contains(&"client/orval.config.ts"));
        assert!(!names.iter().any(|name| name.ends_with(".html")));

        let cargo_config = files
            .iter()
            .find(|(name, _)| *name == ".cargo/config.toml")
            .unwrap()
            .1
            .as_str();
        assert!(cargo_config.contains(r#"dev = "nextrs-dev --bin demo""#));

        let cargo_toml = files
            .iter()
            .find(|(name, _)| *name == "Cargo.toml")
            .unwrap()
            .1
            .as_str();
        assert!(cargo_toml.contains("tower-livereload"));
        assert!(cargo_toml.contains(r#"features = ["vercel"]"#));
        assert!(cargo_toml.contains("vercel_runtime"));
        assert!(!cargo_toml.contains("command-group"));
        assert!(!cargo_toml.contains("ctrlc"));
        assert!(!cargo_toml.contains("ignore"));
        assert!(!cargo_toml.contains("notify-debouncer-full"));
        assert!(!cargo_toml.contains("notify-debouncer-mini"));

        let page = files
            .iter()
            .find(|(name, _)| *name == "app/page.tsx")
            .unwrap()
            .1
            .as_str();
        assert!(page.contains(r#"import { useGetApiPing } from "@demo/client";"#));
        assert!(page.contains("useGetApiPing({ query: { enabled: false } })"));
        assert!(!page.contains(r#"fetch("/api/ping")"#));

        let route = files
            .iter()
            .find(|(name, _)| *name == "app/api/ping/route.rs")
            .unwrap()
            .1
            .as_str();
        assert!(route.contains("#[nextrs::api("));
        assert!(route.contains("ToSchema"));

        let package_json = files
            .iter()
            .find(|(name, _)| *name == "client/package.json")
            .unwrap()
            .1
            .as_str();
        assert!(package_json.contains(r#""gen": "npm run dump && npm run orval""#));
        assert!(package_json.contains(r#""orval": "^7.3.0""#));

        // The client package index re-exports the generated barrel wholesale —
        // the framework rewrites ./generated/index.ts on every build, so no
        // app-side barrel script and no hand-maintained re-export list.
        let index = files
            .iter()
            .find(|(name, _)| *name == "client/src/index.ts")
            .unwrap()
            .1
            .as_str();
        assert!(index.contains(r#"export * from "./generated";"#));
        assert!(!index.contains("./generated/ping/ping"));
        assert!(index.contains("useParams"));
        assert!(!files.iter().any(|(name, _)| name.contains("gen-barrel")));

        // Vercel's default rustc sits below the tsx bundler's MSRV — every
        // generated app needs the toolchain floor or its deploy fails.
        let toolchain = files
            .iter()
            .find(|(name, _)| *name == "rust-toolchain.toml")
            .unwrap()
            .1
            .as_str();
        assert!(toolchain.contains("channel = \"1.96.0\""));

        let vercel = files
            .iter()
            .find(|(name, _)| *name == "vercel.json")
            .unwrap()
            .1
            .as_str();
        assert!(vercel.contains("public, max-age=31536000, immutable"));

        let layout = files
            .iter()
            .find(|(name, _)| *name == "app/layout.tsx")
            .unwrap()
            .1
            .as_str();
        assert!(!layout.contains("/style.css"));
    }

    #[test]
    fn fresh_templates_ship_agents_md() {
        let files = template_files("demo", "@demo/client", &DependencySource::Version);
        let agents = files
            .iter()
            .find(|(name, _)| *name == "AGENTS.md")
            .expect("scaffold generates AGENTS.md")
            .1
            .as_str();
        // The compact agent contract: conventions, no hand-rolling, the
        // bare-import rule, the dev loop, deploys, and the porting pointer.
        assert!(agents.contains("prefetch.rs"));
        assert!(agents.contains("Never hand-roll what the scaffold generates"));
        assert!(agents.contains("client/package.json"));
        assert!(agents.contains("bare import"));
        assert!(agents.contains("cargo dev"));
        assert!(agents.contains("scripts/deploy-prebuilt.sh"));
        assert!(agents.contains("https://nextrs-docs.vercel.app/docs/porting"));
        assert!(agents.contains("server-timing"));
        assert!(agents.contains("nextrs::Timing"));
        assert!(agents.contains("https://nextrs-docs.vercel.app/docs/telemetry"));
        assert!(agents.contains("@demo/client"));
    }

    #[test]
    fn adopt_flag_parses_with_path_and_here() {
        let opts = parse_args(["--adopt".to_string(), "demo".to_string()]).unwrap();
        assert!(opts.adopt);
        assert_eq!(opts.target, Some(PathBuf::from("demo")));

        let opts = parse_args(["--adopt".to_string(), "--here".to_string()]).unwrap();
        assert!(opts.adopt);
        assert!(opts.here);
    }

    #[test]
    fn adopt_templates_are_fresh_wiring_minus_demo_content() {
        let dep = DependencySource::Version;
        let fresh = template_files("demo", "@demo/client", &dep);
        let adopt = adopt_template_files("demo", "@demo/client", &dep);
        let names: Vec<_> = adopt.iter().map(|(name, _)| *name).collect();

        // No demo routes, no demo stylesheet — one minimal page.
        assert!(!names.iter().any(|n| n.starts_with("app/slow")));
        assert!(!names.contains(&"app/api/ping/route.rs"));
        assert!(!names.contains(&"public/style.css"));
        assert!(!names.contains(&"app/layout.tsx"));
        assert!(names.contains(&"app/page.tsx"));
        assert!(names.contains(&"AGENTS.md"));
        assert!(names.contains(&"scripts/deploy-prebuilt.sh"));
        assert!(names.contains(&"vercel.json"));

        // The minimal page must build before any typed client exists.
        let page = &adopt.iter().find(|(n, _)| *n == "app/page.tsx").unwrap().1;
        assert!(!page.contains("import"));

        // Everything shared with the fresh scaffold is byte-identical to it.
        for (name, body) in &adopt {
            if *name == "app/page.tsx" {
                continue;
            }
            let fresh_body = &fresh
                .iter()
                .find(|(n, _)| n == name)
                .unwrap_or_else(|| panic!("{name} missing from fresh templates"))
                .1;
            assert_eq!(body, fresh_body, "{name} diverged from the fresh template");
        }
    }

    #[test]
    fn plan_adopt_file_skips_existing_and_falls_back_for_main_rs() {
        let dir = std::env::temp_dir().join(format!("nextrs-plan-adopt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();

        // Nothing exists: everything is created under its own name.
        assert_eq!(
            plan_adopt_file(&dir, "Cargo.toml"),
            ("Cargo.toml".to_string(), AdoptStatus::Created)
        );
        assert_eq!(
            plan_adopt_file(&dir, "src/main.rs"),
            ("src/main.rs".to_string(), AdoptStatus::Created)
        );

        // Existing files are skipped; an existing main.rs redirects to .example.
        std::fs::write(dir.join("Cargo.toml"), "x").unwrap();
        std::fs::write(dir.join("src/main.rs"), "x").unwrap();
        assert_eq!(
            plan_adopt_file(&dir, "Cargo.toml"),
            ("Cargo.toml".to_string(), AdoptStatus::SkippedExists)
        );
        assert_eq!(
            plan_adopt_file(&dir, "src/main.rs"),
            ("src/main.rs.example".to_string(), AdoptStatus::Created)
        );
        std::fs::write(dir.join("src/main.rs.example"), "x").unwrap();
        assert_eq!(
            plan_adopt_file(&dir, "src/main.rs"),
            ("src/main.rs.example".to_string(), AdoptStatus::SkippedExists)
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn adopt_never_overwrites_existing_files() {
        let dir = std::env::temp_dir().join(format!("nextrs-adopt-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "# preexisting manifest\n").unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "stray file\n").unwrap();

        adopt(&dir, None).unwrap();

        // Pre-seeded files are byte-for-byte untouched.
        assert_eq!(
            std::fs::read_to_string(dir.join("Cargo.toml")).unwrap(),
            "# preexisting manifest\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("src/main.rs")).unwrap(),
            "fn main() {}\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
            "stray file\n"
        );

        // The entrypoint landed beside the existing main.rs instead.
        assert_eq!(
            std::fs::read_to_string(dir.join("src/main.rs.example")).unwrap(),
            main_rs()
        );
        assert!(dir.join("AGENTS.md").exists());
        assert!(dir.join("app/page.tsx").exists());
        assert!(dir.join("build.rs").exists());
        assert!(!dir.join("app/slow").exists());
        assert!(!dir.join("app/api").exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn local_path_dependency_can_be_generated() {
        let toml = cargo_toml(
            "demo",
            &DependencySource::Path(PathBuf::from("/work/nextrs/nextrs")),
        );
        assert!(
            toml.contains(
                r#"nextrs = { path = "/work/nextrs/nextrs", features = ["build", "tsx"] }"#
            )
        );
        assert!(
            toml.contains(r#"nextrs = { path = "/work/nextrs/nextrs", features = ["vercel"] }"#)
        );
    }
}
