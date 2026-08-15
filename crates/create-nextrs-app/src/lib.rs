use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const VERSION: &str = "0.5.0";

/// Run the scaffolder with an explicit argument list.
///
/// This is the shared implementation behind both the legacy
/// `create-nextrs-app` executable and `nextrs new`.
pub fn run_with_args(args: impl IntoIterator<Item = String>) -> io::Result<()> {
    run_with_args_named("create-nextrs-app", args)
}

/// Run the scaffolder under a caller-provided command name.
///
/// The unified CLI uses this to show `nextrs new` in help while the legacy
/// launcher continues to show `create-nextrs-app`.
pub fn run_with_args_named(
    command_name: &str,
    args: impl IntoIterator<Item = String>,
) -> io::Result<()> {
    let options = parse_args(args)?;
    if options.help {
        print_help(command_name);
        return Ok(());
    }
    let target = match options.target {
        Some(path) => path,
        None if options.here => PathBuf::from("."),
        None => prompt_project_path()?,
    };

    if options.adopt {
        adopt(&target, options.nextrs_path.as_deref())?;
    } else {
        scaffold(&target, options.nextrs_path.as_deref(), options.no_install)?;
    }
    Ok(())
}

fn print_help(command_name: &str) {
    println!(
        "{command_name}\n\nUSAGE:\n    {command_name} <path> [--nextrs-path <path>] [--no-install]\n    {command_name} --here [--nextrs-path <path>] [--no-install]\n    {command_name} --adopt [<path> | --here] [--nextrs-path <path>]\n\nCreates a React-first nextrs app with /, /api/ping, and /slow. Fresh apps run\n`npm install` and `npm run client:generate` automatically.\n\nWith --adopt, generates the nextrs skeleton into an EXISTING repo instead:\nminimal content (one page, no demo routes), existing files are never\noverwritten (skipped and reported), an existing src/main.rs gets a\nsrc/main.rs.example beside it, and an existing Cargo.toml is left alone\nwith the dependency lines to merge printed instead. Adopt mode never installs.\n\nOPTIONS:\n    --here                Create the app in the current directory\n    --adopt               Graft the skeleton into an existing directory; never overwrite\n    --no-install          Write a fresh app without installing or generating its client\n    --nextrs-path <path>  Use a local nextrs checkout instead of the published nextrs"
    );
}

#[derive(Debug, Default)]
struct Options {
    target: Option<PathBuf>,
    nextrs_path: Option<PathBuf>,
    here: bool,
    adopt: bool,
    no_install: bool,
    help: bool,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> io::Result<Options> {
    let mut options = Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                options.help = true;
            }
            "--here" => {
                options.here = true;
            }
            "--adopt" => {
                options.adopt = true;
            }
            "--no-install" => {
                options.no_install = true;
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

fn scaffold(target: &Path, nextrs_path: Option<&Path>, no_install: bool) -> io::Result<()> {
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
    if no_install {
        println!("Skipped automatic bootstrap (--no-install).");
        println!("Run these exact commands when ready:");
        for step in bootstrap_steps(target) {
            println!("  {step}");
        }
    } else {
        println!("Installing application dependencies...");
        bootstrap_project(target)?;
        println!();
        println!("Application dependencies and generated client are ready.");
    }

    println!();
    println!("Start the app:");
    if !is_current_dir(target) {
        println!("  cd {}", display_cd_path(target));
    }
    println!("  cargo dev");
    println!();
    println!("Routes:");
    println!("  /          React page");
    println!("  /slow      React page + Rust prefetch + loading.tsx");
    println!("  /api/ping  Rust API route");

    Ok(())
}

/// Install a scaffolded app's root dependencies and generate its typed client.
///
/// Both commands intentionally run at the application root. The hidden
/// generated-client package is an npm workspace and must never be installed
/// independently.
pub fn bootstrap_project(target: &Path) -> io::Result<()> {
    run_bootstrap_with(target, |cwd, args| run_command(cwd, "npm", args))?;
    validate_bootstrapped_client(target)
}

fn run_bootstrap_with(
    target: &Path,
    mut run: impl FnMut(&Path, &[&str]) -> io::Result<()>,
) -> io::Result<()> {
    run(target, &["install"])?;
    run(target, &["run", "client:generate"])
}

fn run_command(cwd: &Path, program: &str, args: &[&str]) -> io::Result<()> {
    let status = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .status()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "failed to run `{program} {}` in {}: {error}",
                    args.join(" "),
                    cwd.display()
                ),
            )
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "`{program} {}` failed with {status} in {}",
            args.join(" "),
            cwd.display()
        )))
    }
}

fn validate_bootstrapped_client(target: &Path) -> io::Result<()> {
    let crate_name = crate_name_from_path(target);
    let package_name = format!("@{crate_name}/client");
    let client_dir = target.join(".nextrs/client");
    let installed_dir = target.join("node_modules").join(&package_name);
    let source_root = std::fs::canonicalize(&client_dir).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "generated client package is missing at {}: {error}",
                client_dir.display()
            ),
        )
    })?;
    let installed_root = std::fs::canonicalize(&installed_dir).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "root npm install did not link {package_name} at {}: {error}",
                installed_dir.display()
            ),
        )
    })?;
    if source_root != installed_root {
        return Err(io::Error::other(format!(
            "root package entry {} does not point to {}; keep the generated app on npm and never install inside .nextrs/client",
            installed_dir.display(),
            client_dir.display()
        )));
    }

    for relative in [
        "package.json",
        "dist/index.js",
        "dist/index.d.ts",
        "dist/react-query.js",
        "dist/react-query.d.ts",
    ] {
        let output = client_dir.join(relative);
        if !output.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "generated client bootstrap did not produce {}",
                    output.display()
                ),
            ));
        }
    }

    let script = format!(
        "await import({package_name:?}); await import({:?})",
        format!("{package_name}/react-query")
    );
    run_command(target, "node", &["--input-type=module", "--eval", &script])
}

fn bootstrap_steps(target: &Path) -> Vec<String> {
    let mut steps = Vec::new();
    if !is_current_dir(target) {
        steps.push(format!("cd {}", display_cd_path(target)));
    }
    steps.push("npm install".to_string());
    steps.push("npm run client:generate".to_string());
    steps
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
        println!("    [lib]");
        println!("    path = \"src/app.rs\"       # shared application Router");
        println!();
        println!("    [[bin]]");
        println!("    name = \"{crate_name}\"   # or your existing binary; set default-run to it");
        println!("    path = \"src/main.rs\"");
        println!();
        println!("    [[bin]]");
        println!("    name = \"{crate_name}-dump-openapi\"");
        println!("    path = \".nextrs/dump-openapi.rs\"");
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
        println!("  src/main.rs.example. Keep application construction in src/app.rs and");
        println!("  make your process entry call {crate_name}::app().");
    } else if skipped("src/main.rs.example") {
        println!();
        println!("  Both src/main.rs and src/main.rs.example already exist — nothing was");
        println!("  written for the entrypoint. See a fresh `nextrs new` app for");
        println!("  the reference main.rs.");
    }
    if skipped(".gitignore") {
        println!();
        println!("  .gitignore was left untouched — make sure it covers:");
        println!("  /target  /public/dist  /node_modules");
        println!("  /.nextrs/client/  /.nextrs/openapi.json  .env");
        println!("  The tracked .nextrs/template/client wiring recreates the ignored");
        println!("  package automatically during generation and `cargo dev`.");
    }
    if skipped("tsconfig.json") {
        println!();
        println!("  tsconfig.json was left untouched. Merge the nextrs TypeScript settings:");
        println!(
            "    - include app/**/*.js, app/**/*.jsx, app/**/*.ts, app/**/*.tsx, and components/**/*.js,jsx,ts,tsx"
        );
        println!("    - set jsx to react-jsx and moduleResolution to Bundler");
        println!(
            "  The generated package has its own config; consume its declarations and do not add paths."
        );
    }
    if skipped("package.json") {
        println!();
        println!("  package.json was left untouched. Add `.nextrs/client` to its npm");
        println!("  workspaces and depend on @{crate_name}/client via");
        println!("  `file:./.nextrs/client`. Keep React, Orval, and TypeScript dependencies");
        println!("  at the root, and copy the client:* scripts from a fresh scaffold.");
    }

    println!();
    println!("  Then, in order:");
    println!("    cargo install cargo-nextrs                  # one-time CLI install");
    println!("    npm install                                 # root only");
    println!("    cargo nextrs client generate                # JS + declarations");
    println!("    cargo dev                                   # build + run with live reload");
    println!();
    println!("  Add API routes as app/**/route.rs with #[nextrs::api], then generate the");
    println!("  typed client: cargo nextrs client generate");
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
                    | "app/PingDemo.tsx"
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
        display_shell_path(path)
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
        ("README.md", readme_md(crate_name, client_alias)),
        ("AGENTS.md", agents_md(crate_name, client_alias)),
        ("build.rs", build_rs(client_alias)),
        ("src/app.rs", app_rs()),
        ("src/main.rs", main_rs(crate_name)),
        (".nextrs/dump-openapi.rs", dump_openapi_rs(crate_name)),
        ("api/index.rs", api_index_rs(crate_name)),
        ("vercel.json", vercel_json()),
        ("scripts/deploy-prebuilt.sh", deploy_prebuilt_sh()),
        ("package.json", root_package_json(crate_name)),
        ("tsconfig.json", root_tsconfig_json()),
        ("app/layout.tsx", layout_tsx()),
        ("app/page.tsx", page_tsx(client_alias)),
        ("app/PingDemo.tsx", ping_demo_tsx(client_alias)),
        ("components/NextrsLogo.tsx", nextrs_logo_tsx()),
        ("app/slow/page.tsx", slow_page_tsx(client_alias)),
        ("app/slow/loading.tsx", slow_loading_tsx()),
        ("app/slow/prefetch.rs", slow_prefetch_rs()),
        ("app/api/ping/route.rs", ping_route_rs()),
        (".nextrs/ensure-client.mjs", ensure_client_mjs()),
        (
            ".nextrs/client/package.json",
            client_package_json(crate_name),
        ),
        (".nextrs/client/orval.config.ts", client_orval_config_ts()),
        (".nextrs/client/tsconfig.json", client_tsconfig_json()),
        (".nextrs/client/src/index.ts", client_index_ts()),
        (".nextrs/client/src/react-query.ts", react_query_index_ts()),
        (".nextrs/client/src/nextrs-client.ts", nextrs_client_ts()),
        (
            ".nextrs/client/scripts/normalize-esm.mjs",
            normalize_esm_mjs(),
        ),
        (
            ".nextrs/template/client/package.json",
            client_package_json(crate_name),
        ),
        (
            ".nextrs/template/client/orval.config.ts",
            client_orval_config_ts(),
        ),
        (
            ".nextrs/template/client/tsconfig.json",
            client_tsconfig_json(),
        ),
        (".nextrs/template/client/src/index.ts", client_index_ts()),
        (
            ".nextrs/template/client/src/react-query.ts",
            react_query_index_ts(),
        ),
        (
            ".nextrs/template/client/src/nextrs-client.ts",
            nextrs_client_ts(),
        ),
        (
            ".nextrs/template/client/scripts/normalize-esm.mjs",
            normalize_esm_mjs(),
        ),
        ("rust-toolchain.toml", rust_toolchain_toml()),
        ("public/style.css", style_css()),
    ]
}

fn readme_md(crate_name: &str, client_alias: &str) -> String {
    format!(
        r#"# {crate_name}

A nextrs application: Rust owns the server and API contract; React owns `.tsx`
pages and components.

## Start developing

Install the CLI once, then use any of the equivalent dev commands:

```sh
cargo install cargo-nextrs
cargo dev
# cargo nextrs dev
# nextrs dev
```

`cargo dev` refreshes the generated client before starting the watcher. Run
`cargo nextrs client generate` explicitly after changing an API when you only
want to refresh types. Install JavaScript dependencies only at this project
root—never inside `.nextrs/`.

## Where code belongs

- `app/`: URL routes and code used by one route. Only convention filenames
  such as `page.tsx`, `layout.tsx`, `prefetch.rs`, and `route.rs` create routes;
  a colocated `TodoRow.tsx` is an ordinary component.
- `components/`: React components shared by multiple routes.
- `src/`: Rust application and domain logic. `src/app.rs` constructs the shared
  Router; `src/main.rs` is the local process entry point.
- `public/`: static files.
- `.nextrs/`: generated framework state. Do not edit it.
- `api/index.rs`: generated Vercel adapter. It contains no application logic.

## Generated API client

A `#[nextrs::api]` Rust handler is exposed through a genuine linked TypeScript
package. Plain fetch functions and React Query integration have separate entry
points:

```ts
import {{ getApiPing }} from "{client_alias}";
import {{ getGetApiPingQueryOptions, useGetApiPing }} from "{client_alias}/react-query";
```

TypeScript and editors resolve both through root `node_modules`; no declaration
shim, relative generated path, or `tsconfig.paths` entry is required.

## Vercel

The default scaffold includes Vercel support because Vercel currently requires
`api/index.rs` as its Rust function entry. Both that adapter and local
`src/main.rs` call `src/app.rs`. If Vercel is not a target, remove
`api/index.rs`, its `index` Cargo target and Vercel-only dependencies, and
`vercel.json` together.
"#
    )
}

fn agents_md(crate_name: &str, client_alias: &str) -> String {
    format!(
        r#"# {crate_name} — contract for coding agents

This is a [nextrs](https://nextrs-docs.vercel.app/docs/getting-started) app:
Rust (Axum) serving Next.js-style file routes with React `.tsx` pages. The
scaffold generated the wiring below — treat it as framework, not app code.

User code belongs in `app/`, `components/`, and `src/`. `.nextrs/` is
framework-owned generated state; import its linked package, never edit it.

## The app/ tree is the router

Directories containing recognized convention files contribute URL segments.
Ordinary `.ts`/`.tsx` modules may be colocated beside a page without creating a
route; put components shared across routes in top-level `components/`.

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
`scripts/deploy-prebuilt.sh`, and `.nextrs/` are generated wiring. Never edit
generated output under `.nextrs/` or `public/dist/`; application seams are
`app/**`, `components/**`, and `src/**`. `src/app.rs` is the shared Rust app,
while `src/main.rs` and `api/index.rs` are process adapters.

## The client package and the bare-import rule

`.nextrs/client` is a real npm workspace package; pages import it as
`{client_alias}` or `{client_alias}/react-query`.

- **Ignore all of `.nextrs/client`.** It is generated state. The tracked
  `.nextrs/template/client` wiring recreates the package before generation;
  never commit or hand-edit its contents.
- **Every bare import used by any `.tsx` file belongs in the root
  `package.json`**. Run `npm install` only at the app root; never install
  dependencies inside `.nextrs/client`.
- **Never hand-write API types.** After changing `#[nextrs::api]` routes, run
  `cargo nextrs client generate` at the app root. The Cargo command owns the
  OpenAPI, Orval, declaration, and package build steps.
  Guide: <https://nextrs-docs.vercel.app/docs/typesafe-client>

## Dev loop

```bash
cargo dev   # build + run + watch (`cargo install cargo-nextrs` once)
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
    concat!(
        "/target\n",
        "/target-vercel\n",
        "/public/dist\n",
        "/node_modules\n",
        "/.vercel\n",
        "/.nextrs/client/\n",
        "/.nextrs/openapi.json\n",
        ".env\n",
        ".env.*.local\n",
        "npm-debug.log*\n",
    )
    .into()
}

fn env_example() -> String {
    "PORT=3000\n".into()
}

fn cargo_config_toml(crate_name: &str) -> String {
    format!(
        r#"[alias]
dev = "nextrs dev --bin {crate_name}"

# vercel-rust reads `config.build.target` whenever this file exists; keep an
# empty build table so host-target Vercel builds do not crash while parsing it.
[build]
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

[lib]
path = "src/app.rs"

[[bin]]
name = "{crate_name}"
path = "src/main.rs"

[[bin]]
name = "{crate_name}-dump-openapi"
path = ".nextrs/dump-openapi.rs"

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
    nextrs::build::emit_registry("app", "src/app.rs", "nextrs_routes.rs")
        .expect("nextrs::build::emit_registry failed");

    nextrs::bundle::bundle_pages(&nextrs::bundle::BundleConfig {{
        app_dir: "app",
        project_dir: Some("."),
        client_dir: ".nextrs/client",
        client_alias: "{client_alias}",
        public_dist: "public/dist",
        ..Default::default()
    }})
    .expect("nextrs::bundle::bundle_pages failed");
}}
"#
    )
}

fn app_rs() -> String {
    r#"//! Shared Rust application construction.
//!
//! Put domain modules beside this file or below `src/`. Both the local
//! process (`main.rs`) and deployment adapters (`api/index.rs`) call `app()`,
//! so application behavior is defined once.

include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

pub fn app() -> axum::Router {
    let public_dir = std::env::var("NEXTRS_PUBLIC_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/public").to_string());

    let app = nextrs::router::build_router_with_public(generated_registry(), &public_dir)
        .merge(nextrs::openapi::spec_router(generated_openapi()));

    #[cfg(debug_assertions)]
    let app = app.layer(tower_livereload::LiveReloadLayer::new());

    app
}
"#
    .into()
}

fn main_rs(crate_name: &str) -> String {
    let crate_module = crate_name.replace('-', "_");
    r#"// Local process entry point. Application routes and domain wiring live in
// src/app.rs so deployment adapters can run the exact same Router.

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let app = APP_CRATE::app();

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
    .replace("APP_CRATE", &crate_module)
}

fn dump_openapi_rs(crate_name: &str) -> String {
    let crate_module = crate_name.replace('-', "_");
    r#"// @generated nextrs helper. Application code belongs in src/app.rs.
fn main() {
    let spec = APP_CRATE::generated_openapi();
    let json = spec.to_pretty_json().expect("serialize OpenAPI document");
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/.nextrs/openapi.json");
    std::fs::write(out, json).expect("write .nextrs/openapi.json");
    eprintln!("wrote {out}");
}
"#
    .replace("APP_CRATE", &crate_module)
}

fn api_index_rs(crate_name: &str) -> String {
    let crate_module = crate_name.replace('-', "_");
    r#"// @generated deployment adapter for Vercel's required api/index.rs entry.
//
// Do not put application logic here: src/app.rs owns the shared Router. If
// this project will not deploy to Vercel, remove this file together with the
// `index` Cargo target, Vercel-only dependencies, and vercel.json.
use nextrs::vercel::StreamingVercelLayer;
use tower::ServiceBuilder;

#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    let app = ServiceBuilder::new()
        .layer(StreamingVercelLayer::new())
        .service(APP_CRATE::app());

    vercel_runtime::run(app).await
}
"#
    .replace("APP_CRATE", &crate_module)
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
  "installCommand": "npm ci",
  "buildCommand": "npm run client:prepare && cargo build --release --bin index && npm run client:build",
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

fn page_tsx(_client_alias: &str) -> String {
    r#"import { NextrsLogo } from "../components/NextrsLogo";
import { PingDemo } from "./PingDemo";

export default function Page() {{
  return (
    <main className="page">
      <section className="panel">
        <NextrsLogo size={72} title="nextrs" />
        <p className="eyebrow">React route</p>
        <h1>Build React apps with Rust routes.</h1>
        <p>
          This page renders immediately in the browser. The button calls a Rust
          route handler at <code>/api/ping</code> through a generated typed client.
        </p>
        <PingDemo />
      </section>
    </main>
  );
}}
"#
    .into()
}

fn ping_demo_tsx(client_alias: &str) -> String {
    format!(
        r#"// Ordinary colocated component: only convention filenames such as
// page.tsx and layout.tsx create routes.
import {{ useGetApiPing }} from "{client_alias}/react-query";

export function PingDemo() {{
  const ping = useGetApiPing({{ query: {{ enabled: false }} }});

  return (
    <>
      <button type="button" onClick={{() => ping.refetch()}} disabled={{ping.isFetching}}>
        {{ping.isFetching ? "Pinging..." : "Ping Rust"}}
      </button>
      <p className="result">{{ping.data?.data.message ?? "Not called yet"}}</p>
    </>
  );
}}
"#
    )
}

fn nextrs_logo_tsx() -> String {
    r##"import type { CSSProperties, SVGProps } from "react";

export interface NextrsLogoProps extends SVGProps<SVGSVGElement> {
  size?: number | string;
  title?: string;
}

export function NextrsLogo({ size = "1em", title, style, ...props }: NextrsLogoProps) {
  return (
    <svg
      viewBox="0 0 64 64"
      width={size}
      height={size}
      role={title ? "img" : undefined}
      aria-hidden={title ? undefined : true}
      style={{ color: "#ef5b2a", ...style } as CSSProperties}
      {...props}
    >
      {title ? <title>{title}</title> : null}
      <circle cx="32" cy="32" r="25" fill="none" stroke="currentColor" strokeWidth="6" />
      <path d="M32 14 48 43H16Z" fill="currentColor">
        <animateTransform attributeName="transform" type="rotate" from="0 32 32" to="360 32 32" dur="8s" repeatCount="indefinite" />
      </path>
      <circle cx="32" cy="32" r="5" fill="white" />
    </svg>
  );
}
"##
    .into()
}

fn slow_page_tsx(client_alias: &str) -> String {
    format!(
        r#"import {{ useSeed }} from "{client_alias}/react-query";

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

#[nextrs::api]
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
  "description": "Generated framework-agnostic and React Query clients for {crate_name}.",
  "type": "module",
  "sideEffects": false,
  "main": "./dist/index.js",
  "types": "./dist/index.d.ts",
  "exports": {{
    ".": {{
      "types": "./dist/index.d.ts",
      "import": "./dist/index.js",
      "default": "./dist/index.js"
    }},
    "./react-query": {{
      "types": "./dist/react-query.d.ts",
      "import": "./dist/react-query.js",
      "default": "./dist/react-query.js"
    }}
  }},
  "files": [
    "dist"
  ],
  "peerDependencies": {{
    "@tanstack/react-query": "^5.62.0",
    "@tanstack/react-router": "^1.87.0",
    "react": "^19.0.0"
  }},
  "peerDependenciesMeta": {{
    "@tanstack/react-query": {{
      "optional": true
    }},
    "@tanstack/react-router": {{
      "optional": true
    }},
    "react": {{
      "optional": true
    }}
  }}
}}
"#
    )
}

fn root_package_json(crate_name: &str) -> String {
    format!(
        r#"{{
  "name": "{crate_name}-app",
  "private": true,
  "workspaces": [".nextrs/client"],
  "scripts": {{
    "client:ensure": "node .nextrs/ensure-client.mjs",
    "client:dump": "NEXTRS_SKIP_BUNDLE=1 cargo run --bin {crate_name}-dump-openapi",
    "client:orval": "cd .nextrs/client && orval --config ./orval.config.ts",
    "client:build": "tsc --project .nextrs/client/tsconfig.json && node .nextrs/client/scripts/normalize-esm.mjs",
    "client:prepare": "npm run client:ensure && npm run client:dump && npm run client:orval",
    "client:generate": "npm run client:prepare && cargo build && npm run client:build",
    "typecheck": "tsc --project tsconfig.json"
  }},
  "dependencies": {{
    "@{crate_name}/client": "file:./.nextrs/client",
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
"#,
    )
}

fn client_orval_config_ts() -> String {
    r#"import { defineConfig } from "orval";

export default defineConfig({
  fetch: {
    input: "../openapi.json",
    output: {
      mode: "tags-split",
      target: "./src/generated/fetch",
      schemas: "./src/generated/fetch/model",
      client: "fetch",
      httpClient: "fetch",
      baseUrl: "/",
      clean: true,
      prettier: false,
    },
  },
  reactQuery: {
    input: "../openapi.json",
    output: {
      mode: "tags-split",
      target: "./src/generated/react-query",
      schemas: "./src/generated/react-query/model",
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

fn client_tsconfig_json() -> String {
    r#"{
  "compilerOptions": {
    "target": "ES2020",
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "strict": true,
    "skipLibCheck": true,
    "declaration": true,
    "rootDir": "src",
    "outDir": "dist"
  },
  "include": ["src/**/*.ts"]
}
"#
    .into()
}

fn root_tsconfig_json() -> String {
    r#"{
  "compilerOptions": {
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
    "allowJs": true,
    "checkJs": true
  },
  "include": [
    "app/**/*.js",
    "app/**/*.jsx",
    "app/**/*.ts",
    "app/**/*.tsx",
    "components/**/*.js",
    "components/**/*.jsx",
    "components/**/*.ts",
    "components/**/*.tsx"
  ]
}
"#
    .into()
}

fn client_index_ts() -> String {
    r#"// Framework-agnostic fetch functions and wire types.
// @generated API modules are refreshed by `nextrs client generate`.
export * from "./generated/fetch";
"#
    .into()
}

fn ensure_client_mjs() -> String {
    r#"// Materialize the ignored generated-client package from tracked framework wiring.
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const nextrsDir = dirname(fileURLToPath(import.meta.url));
const rootDir = resolve(nextrsDir, "..");
const templateDir = join(nextrsDir, "template", "client");
const clientDir = join(nextrsDir, "client");
const rootPackage = JSON.parse(await readFile(join(rootDir, "package.json"), "utf8"));
const dependencies = {
  ...rootPackage.dependencies,
  ...rootPackage.devDependencies,
  ...rootPackage.optionalDependencies,
};
const clientEntry = Object.entries(dependencies).find(([, value]) =>
  typeof value === "string" &&
  value.replace(/^file:/, "").replace(/^\.\//, "").replace(/\/$/, "") === ".nextrs/client"
);
if (!clientEntry) {
  throw new Error("package.json must depend on the generated client via file:./.nextrs/client");
}
const [clientName] = clientEntry;

async function writeIfChanged(path, contents) {
  let current;
  try {
    current = await readFile(path, "utf8");
  } catch {}
  if (current === contents) return;
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, contents);
}

async function materialize(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const source = join(dir, entry.name);
    if (entry.isDirectory()) {
      await materialize(source);
      continue;
    }
    const rel = relative(templateDir, source);
    let contents = await readFile(source, "utf8");
    if (rel === "package.json") {
      const manifest = JSON.parse(contents);
      manifest.name = clientName;
      contents = `${JSON.stringify(manifest, null, 2)}\n`;
    }
    await writeIfChanged(join(clientDir, rel), contents);
  }
}

await materialize(templateDir);
"#
    .into()
}

fn normalize_esm_mjs() -> String {
    r#"// TypeScript's Bundler module resolution intentionally preserves extensionless
// relative specifiers. Node's ESM loader requires explicit files, so make the
// emitted package portable after tsc without changing Orval-owned source.
import { readdir, readFile, stat, writeFile } from "node:fs/promises";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const clientDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const distDir = join(clientDir, "dist");

async function isFile(path) {
  try {
    return (await stat(path)).isFile();
  } catch {
    return false;
  }
}

async function emittedSpecifier(file, specifier) {
  if (extname(specifier)) return specifier;
  const target = resolve(dirname(file), specifier);
  if (await isFile(`${target}.js`)) return `${specifier}.js`;
  if (await isFile(join(target, "index.js"))) {
    return `${specifier.replace(/\/$/, "")}/index.js`;
  }
  return specifier;
}

async function filesUnder(dir) {
  const files = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) files.push(...(await filesUnder(path)));
    else if (entry.isFile() && (path.endsWith(".js") || path.endsWith(".d.ts"))) {
      files.push(path);
    }
  }
  return files;
}

function moduleSpecifiers(file, source) {
  const sourceFile = ts.createSourceFile(
    file,
    source,
    ts.ScriptTarget.Latest,
    true,
  );
  const specifiers = [];
  const add = (literal) => {
    if (literal?.text.startsWith(".")) {
      specifiers.push({
        start: literal.getStart(sourceFile) + 1,
        end: literal.getEnd() - 1,
        value: literal.text,
      });
    }
  };
  const visit = (node) => {
    if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) {
      add(node.moduleSpecifier);
    } else if (
      ts.isCallExpression(node) &&
      node.expression.kind === ts.SyntaxKind.ImportKeyword &&
      ts.isStringLiteral(node.arguments[0])
    ) {
      add(node.arguments[0]);
    } else if (
      ts.isImportTypeNode(node) &&
      ts.isLiteralTypeNode(node.argument) &&
      ts.isStringLiteral(node.argument.literal)
    ) {
      add(node.argument.literal);
    }
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
  return specifiers;
}

let rewritten = 0;
const anyTypes = [];
const emittedFiles = await filesUnder(distDir);
for (const file of emittedFiles) {
  const source = await readFile(file, "utf8");
  let output = "";
  let cursor = 0;
  for (const moduleSpecifier of moduleSpecifiers(file, source)) {
    const specifier = await emittedSpecifier(file, moduleSpecifier.value);
    output += source.slice(cursor, moduleSpecifier.start);
    output += specifier;
    cursor = moduleSpecifier.end;
    if (specifier !== moduleSpecifier.value) rewritten += 1;
  }
  output += source.slice(cursor);
  if (output !== source) await writeFile(file, output);

  if (file.endsWith(".d.ts")) {
    const declaration = ts.createSourceFile(
      file,
      output,
      ts.ScriptTarget.Latest,
      true,
    );
    const visit = (node) => {
      if (node.kind === ts.SyntaxKind.AnyKeyword) {
        const { line, character } = declaration.getLineAndCharacterOfPosition(
          node.getStart(declaration),
        );
        anyTypes.push(`${file}:${line + 1}:${character + 1}`);
      }
      ts.forEachChild(node, visit);
    };
    visit(declaration);
  }
}

if (anyTypes.length > 0) {
  throw new Error(
    `generated public declarations contain explicit any types:\n${anyTypes.join("\n")}`,
  );
}

// Package self-references exercise the real exports map, not a convenient
// relative file path. Keep this check in every client build.
const packageJson = JSON.parse(
  await readFile(join(clientDir, "package.json"), "utf8"),
);
await import(packageJson.name);
await import(`${packageJson.name}/react-query`);
console.log(
  `normalized ${rewritten} ESM specifiers; verified package exports and ${emittedFiles.filter((file) => file.endsWith(".d.ts")).length} declarations without any`,
);

export { emittedSpecifier, moduleSpecifiers };
"#
    .into()
}

fn react_query_index_ts() -> String {
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

// React Query hooks, option factories, query keys, and URL-bound helpers.
export * from "./generated/react-query";
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
        assert!(!opts.no_install);
    }

    #[test]
    fn no_install_flag_skips_automatic_bootstrap() {
        let opts = parse_args(["demo".to_string(), "--no-install".to_string()]).unwrap();
        assert_eq!(opts.target, Some(PathBuf::from("demo")));
        assert!(opts.no_install);
    }

    #[test]
    fn bootstrap_runs_both_commands_at_the_application_root() {
        let root = Path::new("/tmp/demo");
        let mut calls = Vec::new();
        run_bootstrap_with(root, |cwd, args| {
            calls.push((cwd.to_path_buf(), args.join(" ")));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            calls,
            [
                (root.to_path_buf(), "install".to_string()),
                (root.to_path_buf(), "run client:generate".to_string()),
            ]
        );
    }

    #[test]
    fn no_install_steps_are_exact_and_shell_safe() {
        assert_eq!(
            bootstrap_steps(Path::new("My App")),
            [
                "cd 'My App'".to_string(),
                "npm install".to_string(),
                "npm run client:generate".to_string(),
            ]
        );
    }

    #[test]
    fn fresh_scaffold_creates_precise_gitignore() {
        let dir = std::env::temp_dir().join(format!(
            "nextrs-scaffold-gitignore-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        scaffold(&dir, None, true).unwrap();

        let contents = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert_eq!(contents, gitignore());
        assert!(contents.lines().any(|line| line == "/.nextrs/client/"));
        assert!(contents.lines().any(|line| line == "/.nextrs/openapi.json"));

        // The actual package is ignored. Its small framework-owned template
        // survives a commit/clone and recreates it before generation.
        for required in [
            ".nextrs/ensure-client.mjs",
            ".nextrs/template/client/package.json",
            ".nextrs/template/client/orval.config.ts",
            ".nextrs/template/client/tsconfig.json",
            ".nextrs/template/client/src/index.ts",
            ".nextrs/template/client/src/react-query.ts",
            ".nextrs/template/client/src/nextrs-client.ts",
            ".nextrs/template/client/scripts/normalize-esm.mjs",
        ] {
            assert!(dir.join(required).is_file(), "{required} was not created");
        }

        std::fs::remove_dir_all(&dir).unwrap();
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
        assert!(names.contains(&"tsconfig.json"));
        assert!(names.contains(&"package.json"));
        assert!(names.contains(&"src/app.rs"));
        assert!(names.contains(&".nextrs/dump-openapi.rs"));
        assert!(names.contains(&"api/index.rs"));
        assert!(names.contains(&"vercel.json"));
        assert!(names.contains(&"app/layout.tsx"));
        assert!(names.contains(&"app/page.tsx"));
        assert!(names.contains(&"app/PingDemo.tsx"));
        assert!(names.contains(&"components/NextrsLogo.tsx"));
        assert!(names.contains(&"app/slow/loading.tsx"));
        assert!(names.contains(&"app/slow/prefetch.rs"));
        assert!(names.contains(&"app/api/ping/route.rs"));
        assert!(names.contains(&".nextrs/client/orval.config.ts"));
        assert!(names.contains(&".nextrs/client/tsconfig.json"));
        assert!(names.contains(&".nextrs/client/scripts/normalize-esm.mjs"));
        assert!(!names.iter().any(|name| name.starts_with("client/")));
        assert!(!names.iter().any(|name| name.contains("external")));
        assert!(!names.iter().any(|name| name.ends_with(".html")));

        let cargo_config = files
            .iter()
            .find(|(name, _)| *name == ".cargo/config.toml")
            .unwrap()
            .1
            .as_str();
        assert!(cargo_config.contains(r#"dev = "nextrs dev --bin demo""#));
        assert!(cargo_config.contains("[build]"));

        let cargo_toml = files
            .iter()
            .find(|(name, _)| *name == "Cargo.toml")
            .unwrap()
            .1
            .as_str();
        assert!(cargo_toml.contains(r#"nextrs = { version = "0.5.0", features"#));
        assert!(cargo_toml.contains("tower-livereload"));
        assert!(cargo_toml.contains(r#"features = ["vercel"]"#));
        assert!(cargo_toml.contains("vercel_runtime"));
        assert!(cargo_toml.contains(r#"path = "src/app.rs""#));
        assert!(cargo_toml.contains(r#"path = ".nextrs/dump-openapi.rs""#));
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
        assert!(page.contains(r#"import { NextrsLogo } from "../components/NextrsLogo";"#));
        assert!(page.contains(r#"import { PingDemo } from "./PingDemo";"#));
        assert!(!page.contains(r#"fetch("/api/ping")"#));

        let colocated = files
            .iter()
            .find(|(name, _)| *name == "app/PingDemo.tsx")
            .unwrap()
            .1
            .as_str();
        assert!(colocated.contains(r#"from "@demo/client/react-query""#));

        let route = files
            .iter()
            .find(|(name, _)| *name == "app/api/ping/route.rs")
            .unwrap()
            .1
            .as_str();
        assert!(route.contains("#[nextrs::api]"));
        assert!(route.contains("ToSchema"));

        let package_json = files
            .iter()
            .find(|(name, _)| *name == ".nextrs/client/package.json")
            .unwrap()
            .1
            .as_str();
        assert!(package_json.contains(r#""types": "./dist/index.d.ts""#));
        assert!(package_json.contains(r#""./react-query""#));
        assert!(package_json.contains(r#""import": "./dist/react-query.js""#));
        assert!(package_json.contains(r#""sideEffects": false"#));
        assert!(package_json.contains(r#""peerDependenciesMeta""#));

        let root_package = files
            .iter()
            .find(|(name, _)| *name == "package.json")
            .unwrap()
            .1
            .as_str();
        assert!(root_package.contains(r#""workspaces": [".nextrs/client"]"#));
        assert!(root_package.contains(r#""@demo/client": "file:./.nextrs/client""#));
        assert!(root_package.contains(r#""client:generate""#));
        assert!(root_package.contains(
            "tsc --project .nextrs/client/tsconfig.json && node .nextrs/client/scripts/normalize-esm.mjs"
        ));

        let client_tsconfig = files
            .iter()
            .find(|(name, _)| *name == ".nextrs/client/tsconfig.json")
            .unwrap()
            .1
            .as_str();
        assert!(client_tsconfig.contains(r#""declaration": true"#));
        assert!(!client_tsconfig.contains("declarationMap"));
        assert!(!client_tsconfig.contains("sourceMap"));

        let normalizer = files
            .iter()
            .find(|(name, _)| *name == ".nextrs/client/scripts/normalize-esm.mjs")
            .unwrap()
            .1
            .as_str();
        assert!(normalizer.contains(r#"return `${specifier}.js`"#));
        assert!(normalizer.contains(r#"/index.js`"#));
        assert!(normalizer.contains("await import(packageJson.name)"));
        assert!(normalizer.contains(r#"await import(`${packageJson.name}/react-query`)"#));
        assert!(normalizer.contains("ts.SyntaxKind.AnyKeyword"));

        let ignored = files
            .iter()
            .find(|(name, _)| *name == ".gitignore")
            .unwrap()
            .1
            .as_str();
        assert!(ignored.contains("/.nextrs/client/"));
        assert!(ignored.contains("/.nextrs/openapi.json"));

        let tsconfig = files
            .iter()
            .find(|(name, _)| *name == "tsconfig.json")
            .unwrap()
            .1
            .as_str();
        assert!(!tsconfig.contains(r#""paths""#));
        assert!(tsconfig.contains(r#""allowJs": true"#));
        assert!(tsconfig.contains(r#""checkJs": true"#));
        assert!(tsconfig.contains(r#""app/**/*.js""#));
        assert!(tsconfig.contains(r#""app/**/*.tsx""#));
        assert!(tsconfig.contains(r#""components/**/*.tsx""#));
        assert!(!tsconfig.contains(r#"".nextrs/client/src/**/*.ts""#));
        assert!(!tsconfig.contains("orval.config.ts"));
        assert!(!tsconfig.contains(r#""paths""#));

        // The client package index re-exports the generated barrel wholesale —
        // the framework rewrites ./generated/index.ts on every build, so no
        // app-side barrel script and no hand-maintained re-export list.
        let index = files
            .iter()
            .find(|(name, _)| *name == ".nextrs/client/src/index.ts")
            .unwrap()
            .1
            .as_str();
        assert!(index.contains(r#"export * from "./generated/fetch";"#));
        assert!(!index.contains("@tanstack/react-query"));
        let react_query = files
            .iter()
            .find(|(name, _)| *name == ".nextrs/client/src/react-query.ts")
            .unwrap()
            .1
            .as_str();
        assert!(react_query.contains(r#"export * from "./generated/react-query";"#));
        assert!(react_query.contains("useParams"));
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
        assert!(agents.contains(".nextrs"));
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
            (
                "src/main.rs.example".to_string(),
                AdoptStatus::SkippedExists
            )
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn adopt_never_overwrites_existing_files() {
        let dir = std::env::temp_dir().join(format!("nextrs-adopt-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "# preexisting manifest\n").unwrap();
        std::fs::write(dir.join(".gitignore"), "# user-owned\n/cache/\n").unwrap();
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
            std::fs::read_to_string(dir.join(".gitignore")).unwrap(),
            "# user-owned\n/cache/\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
            "stray file\n"
        );

        // The entrypoint landed beside the existing main.rs instead.
        assert_eq!(
            std::fs::read_to_string(dir.join("src/main.rs.example")).unwrap(),
            main_rs(&crate_name_from_path(&dir))
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
