use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const VERSION: &str = "0.3";

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
        None => prompt_project_path()?,
    };

    scaffold(&target, options.nextrs_path.as_deref())?;
    Ok(())
}

fn print_help() {
    println!(
        "create-nextrs-app\n\nUSAGE:\n    create-nextrs-app <path> [--nextrs-path <path>]\n\nCreates a React-first nextrs app with /, /api/ping, and /slow.\n\nOPTIONS:\n    --nextrs-path <path>  Use a local nextrs checkout instead of nextrs = \"0.3\""
    );
}

#[derive(Debug, Default)]
struct Options {
    target: Option<PathBuf>,
    nextrs_path: Option<PathBuf>,
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
        std::fs::write(path, body)?;
    }

    println!("Created {}", target.display());
    println!();
    println!("Next steps:");
    println!("  cd {}", display_cd_path(target));
    println!("  cd client && npm install && npm run gen && cd ..");
    println!("  cargo dev");
    println!();
    println!("Routes:");
    println!("  /          React page");
    println!("  /slow      React page + Rust props + loading.tsx");
    println!("  /api/ping  Rust API route");

    Ok(())
}

fn display_cd_path(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        ".".to_string()
    } else {
        path.display().to_string()
    }
}

fn crate_name_from_path(path: &Path) -> String {
    let raw = path
        .file_name()
        .and_then(OsStr::to_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("nextrs-app");

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
            Self::Version => format!(r#""{VERSION}""#),
            Self::Path(path) => format!(
                r#"{{ path = "{}" }}"#,
                toml_string(&path.display().to_string())
            ),
        }
    }
}

fn toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn template_files(
    crate_name: &str,
    client_alias: &str,
    dep: &DependencySource,
) -> Vec<(&'static str, String)> {
    vec![
        (".gitignore", gitignore()),
        (".cargo/config.toml", cargo_config_toml()),
        ("Cargo.toml", cargo_toml(crate_name, dep)),
        ("build.rs", build_rs(client_alias)),
        ("src/main.rs", main_rs()),
        ("src/bin/dev.rs", dev_rs()),
        ("src/bin/dump-openapi.rs", dump_openapi_rs()),
        ("app/layout.tsx", layout_tsx()),
        ("app/page.tsx", page_tsx(client_alias)),
        ("app/slow/page.tsx", slow_page_tsx(client_alias)),
        ("app/slow/loading.tsx", slow_loading_tsx()),
        ("app/slow/props.rs", slow_props_rs()),
        ("app/api/ping/route.rs", ping_route_rs()),
        ("client/package.json", client_package_json(crate_name)),
        ("client/orval.config.ts", client_orval_config_ts()),
        ("client/tsconfig.json", client_tsconfig_json(client_alias)),
        ("client/src/index.ts", client_index_ts()),
        ("client/src/nextrs-client.ts", nextrs_client_ts()),
        ("public/style.css", style_css()),
    ]
}

fn gitignore() -> String {
    "/target\n/public/dist\n/node_modules\n/client/node_modules\n.env\n".into()
}

fn cargo_config_toml() -> String {
    r#"[alias]
dev = "run --bin nextrs-dev --"
"#
    .into()
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
name = "nextrs-dev"
path = "src/bin/dev.rs"

[build-dependencies]
nextrs = {build_dependency}

[dependencies]
nextrs = {runtime_dependency}
axum = "0.8"
tokio = {{ version = "1", features = ["full"] }}
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
    let public_dir = std::env::var("NEXTRS_PUBLIC_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/public").to_string());

    let app = nextrs::router::build_router_with_public(generated_registry(), &public_dir)
        .merge(nextrs::openapi::spec_router(generated_openapi()));

    #[cfg(debug_assertions)]
    let app = app.layer(tower_livereload::LiveReloadLayer::new());

    let addr = format!(
        "0.0.0.0:{}",
        std::env::var("PORT").unwrap_or_else(|_| "3000".to_string())
    );
    println!("listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
"#
    .into()
}

fn dev_rs() -> String {
    r#"use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileState {
    modified: Option<SystemTime>,
    len: u64,
}

type Snapshot = BTreeMap<PathBuf, FileState>;

const WATCH_PATHS: &[&str] = &[
    ".cargo/config.toml",
    "Cargo.lock",
    "Cargo.toml",
    "app",
    "build.rs",
    "client/package-lock.json",
    "client/package.json",
    "client/src",
    "client/tsconfig.json",
    "public",
    "src",
];

fn main() -> std::io::Result<()> {
    let root = env::current_dir()?;
    let command = command_from_args();

    eprintln!("nextrs-dev watching {} paths", WATCH_PATHS.len());
    eprintln!("nextrs-dev command: {}", display_command(&command));

    let mut current = snapshot(&root)?;
    let mut child = spawn(&command)?;

    loop {
        thread::sleep(Duration::from_millis(500));

        if let Some(status) = child.try_wait()? {
            eprintln!("nextrs-dev child exited with {status}; waiting for changes");
            wait_for_change(&root, &mut current)?;
            child = spawn(&command)?;
            continue;
        }

        let next = snapshot(&root)?;
        if next != current {
            thread::sleep(Duration::from_millis(150));
            current = snapshot(&root)?;
            eprintln!("nextrs-dev change detected; restarting");
            stop(&mut child)?;
            child = spawn(&command)?;
        }
    }
}

fn command_from_args() -> Vec<OsString> {
    let mut args = env::args_os().skip(1);
    if matches!(args.next().as_deref(), Some(arg) if arg == "--") {
        let command: Vec<_> = args.collect();
        if !command.is_empty() {
            return command;
        }
    }

    vec![
        "cargo".into(),
        "run".into(),
        "--bin".into(),
        env!("CARGO_PKG_NAME").into(),
    ]
}

fn display_command(command: &[OsString]) -> String {
    command
        .iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

fn wait_for_change(root: &Path, current: &mut Snapshot) -> std::io::Result<()> {
    loop {
        thread::sleep(Duration::from_millis(500));
        let next = snapshot(root)?;
        if next != *current {
            *current = next;
            return Ok(());
        }
    }
}

fn snapshot(root: &Path) -> std::io::Result<Snapshot> {
    let mut files = Snapshot::new();
    for rel in WATCH_PATHS {
        collect_path(&root.join(rel), &mut files)?;
    }
    Ok(files)
}

fn collect_path(path: &Path, files: &mut Snapshot) -> std::io::Result<()> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };

    if metadata.is_file() {
        files.insert(
            path.to_path_buf(),
            FileState {
                modified: metadata.modified().ok(),
                len: metadata.len(),
            },
        );
        return Ok(());
    }

    if metadata.is_dir() {
        if path.ends_with("node_modules") || path.ends_with("target") {
            return Ok(());
        }
        if path.ends_with("dist") && path.parent().is_some_and(|p| p.ends_with("public")) {
            return Ok(());
        }
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            collect_path(&entry.path(), files)?;
        }
    }

    Ok(())
}

fn spawn(command: &[OsString]) -> std::io::Result<Child> {
    let mut cmd = Command::new(&command[0]);
    cmd.args(&command[1..])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    cmd.spawn()
}

fn stop(child: &mut Child) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let pgid = format!("-{}", child.id());
        let _ = Command::new("kill").arg("-TERM").arg(&pgid).status();

        for _ in 0..20 {
            if child.try_wait()?.is_some() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }

        let _ = Command::new("kill").arg("-KILL").arg(&pgid).status();
        let _ = child.wait();
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        child.kill()?;
        let _ = child.wait();
        Ok(())
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

fn layout_tsx() -> String {
    r#"import type { ReactNode } from "react";

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <div className="app-shell">
      <link rel="stylesheet" href="/style.css" />
      <header className="topbar">
        <a href="/" className="brand">nextrs</a>
        <nav>
          <a href="/">Home</a>
          <a href="/slow">Slow props</a>
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
        <p className="eyebrow">Server props</p>
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
        <p className="eyebrow">Server props</p>
        <h1>Loading from Rust...</h1>
        <p>This route waits two seconds in <code>props.rs</code>.</p>
      </section>
    </main>
  );
}
"#
    .into()
}

fn slow_props_rs() -> String {
    r#"use std::time::Duration;

pub async fn props(_req: http::Request<axum::body::Body>) -> nextrs::QuerySeed {
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

export function useSeed<T>(key: unknown[]): T | undefined {
  return useQueryClient().getQueryData<{ data: T }>(key)?.data;
}

export * from "./generated/ping/ping";
export * from "./generated/model";
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
    fn templates_use_react_first_conventions() {
        let files = template_files("demo", "@demo/client", &DependencySource::Version);
        let names: Vec<_> = files.iter().map(|(name, _)| *name).collect();
        assert!(names.contains(&".cargo/config.toml"));
        assert!(names.contains(&"src/bin/dev.rs"));
        assert!(names.contains(&"src/bin/dump-openapi.rs"));
        assert!(names.contains(&"app/layout.tsx"));
        assert!(names.contains(&"app/page.tsx"));
        assert!(names.contains(&"app/slow/loading.tsx"));
        assert!(names.contains(&"app/slow/props.rs"));
        assert!(names.contains(&"app/api/ping/route.rs"));
        assert!(names.contains(&"client/orval.config.ts"));
        assert!(!names.iter().any(|name| name.ends_with(".html")));

        let cargo_config = files
            .iter()
            .find(|(name, _)| *name == ".cargo/config.toml")
            .unwrap()
            .1
            .as_str();
        assert!(cargo_config.contains(r#"dev = "run --bin nextrs-dev --""#));

        let cargo_toml = files
            .iter()
            .find(|(name, _)| *name == "Cargo.toml")
            .unwrap()
            .1
            .as_str();
        assert!(cargo_toml.contains("tower-livereload"));

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
        assert!(toml.contains(r#"nextrs = { path = "/work/nextrs/nextrs" }"#));
    }
}
