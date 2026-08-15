use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde_json::Value;

const DEFAULT_CLIENT_DIR: &str = ".nextrs/client";

/// Run a nextrs CLI launcher and translate its result into a process exit code.
///
/// `command_name` only controls the error prefix, allowing `cargo nextrs` and
/// `nextrs` to share the exact same command implementation.
pub fn main_with_args(command_name: &str, args: impl IntoIterator<Item = OsString>) -> ExitCode {
    match run_with_args(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{command_name}: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Dispatch a nextrs CLI argument list.
pub fn run_with_args(args: impl IntoIterator<Item = OsString>) -> Result<(), String> {
    let command = CommandLine::parse(args)?;
    match command {
        CommandLine::Help => {
            print_help();
            Ok(())
        }
        CommandLine::New(args) => create_app(args),
        CommandLine::Dev(args) => {
            prepare_generated_client_for_dev()?;
            cargo_nextrs_dev::run_with_args(args).map_err(io_error)
        }
        CommandLine::ClientGenerate(options) => generate_client(options),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CommandLine {
    Help,
    New(Vec<OsString>),
    Dev(Vec<OsString>),
    ClientGenerate(GenerateOptions),
}

#[derive(Debug, PartialEq, Eq)]
struct GenerateOptions {
    root: PathBuf,
    client_dir: PathBuf,
    config: Option<PathBuf>,
}

impl CommandLine {
    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut args = args.into_iter().peekable();
        if matches!(args.peek().map(OsString::as_os_str), Some(arg) if arg == "nextrs") {
            args.next();
        }

        let Some(first) = args.next() else {
            return Ok(Self::Help);
        };
        if matches!(first.to_str(), Some("-h" | "--help" | "help")) {
            return Ok(Self::Help);
        }
        if first == "new" {
            return Ok(Self::New(args.collect()));
        }
        if first == "dev" {
            return Ok(Self::Dev(args.collect()));
        }
        if first != "client" {
            return Err(format!("unknown command `{}`", first.to_string_lossy()));
        }

        let Some(action) = args.next() else {
            return Err("missing client command; expected `generate`".into());
        };
        if action != "generate" {
            return Err(format!(
                "unknown client command `{}`; expected `generate`",
                action.to_string_lossy()
            ));
        }

        let mut root = PathBuf::from(".");
        let mut client_dir = PathBuf::from(DEFAULT_CLIENT_DIR);
        let mut config = None;
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--root") => root = required_path(&mut args, "--root")?,
                Some("--client-dir") => client_dir = required_path(&mut args, "--client-dir")?,
                Some("--config") => config = Some(required_path(&mut args, "--config")?),
                Some("-h" | "--help") => return Ok(Self::Help),
                Some(flag) if flag.starts_with('-') => {
                    return Err(format!("unknown option `{flag}`"));
                }
                _ => return Err(format!("unexpected argument `{}`", arg.to_string_lossy())),
            }
        }

        Ok(Self::ClientGenerate(GenerateOptions {
            root,
            client_dir,
            config,
        }))
    }
}

fn create_app(args: Vec<OsString>) -> Result<(), String> {
    let args = args
        .into_iter()
        .map(|arg| {
            arg.into_string()
                .map_err(|arg| format!("new arguments must be valid UTF-8: {arg:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    create_nextrs_app::run_with_args_named("nextrs new", args).map_err(io_error)
}

fn required_path(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<PathBuf, String> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("{flag} requires a path"))
}

fn generate_client(options: GenerateOptions) -> Result<(), String> {
    let root = absolutize(&env::current_dir().map_err(io_error)?, &options.root);
    let root_package_json = root.join("package.json");
    if !root_package_json.is_file() {
        return Err(format!(
            "{} does not exist; run this from a nextrs app root or pass --root",
            root_package_json.display()
        ));
    }

    let custom_client_dir = options.client_dir != Path::new(DEFAULT_CLIENT_DIR);
    let requested_client_dir = absolutize(&root, &options.client_dir);
    if !custom_client_dir
        && !requested_client_dir.join("package.json").is_file()
        && root_declares_generated_client(&root_package_json)?
    {
        eprintln!("nextrs: materializing the ignored generated-client package");
        execute(&root, "npm", &["run", "client:ensure"], None)?;
    }
    let legacy_client_dir = root.join("client");
    let client_dir = if !custom_client_dir
        && !requested_client_dir.join("package.json").is_file()
        && legacy_client_dir.join("package.json").is_file()
    {
        eprintln!(
            "nextrs: using legacy client directory {}; regenerate the app scaffold to move it to {}",
            legacy_client_dir.display(),
            requested_client_dir.display()
        );
        legacy_client_dir
    } else {
        requested_client_dir
    };
    let package_json = client_dir.join("package.json");
    if !package_json.is_file() {
        return Err(format!(
            "{} does not exist after client materialization; restore `.nextrs/ensure-client.mjs` and `.nextrs/template/client` from a fresh `nextrs new` app, or pass --client-dir for a legacy client",
            package_json.display(),
        ));
    }

    let modern_package = if !custom_client_dir && client_dir == root.join(DEFAULT_CLIENT_DIR) {
        let package = read_client_package(&package_json)?;
        validate_root_client_contract(&root_package_json, &package.name)?;
        warn_on_mixed_package_managers(&root);
        ensure_root_client_install(&root, &client_dir, &package)?;
        Some(package)
    } else {
        if !root.join("node_modules").is_dir() {
            eprintln!("nextrs: installing application dependencies at the app root");
            execute(&root, "npm", &["install"], None)?;
        }
        None
    };

    let default_config = client_dir.join("nextrs.client.json");
    let config = options
        .config
        .map(|path| absolutize(&root, &path))
        .or_else(|| default_config.is_file().then_some(default_config));

    if let Some(config) = config {
        if !config.is_file() {
            return Err(format!(
                "external client config not found: {}",
                config.display()
            ));
        }
        eprintln!(
            "nextrs: generating internal client and publishing external client from {}",
            config.display()
        );
        let config_arg = config.as_os_str();
        execute(
            &client_dir,
            "npm",
            &[
                OsStr::new("run"),
                OsStr::new("generate:external"),
                OsStr::new("--"),
                config_arg,
            ],
            None,
        )?;
    } else {
        eprintln!("nextrs: generating client from the current Rust contract");
        let (cwd, script) = normal_generation_target(&root, &client_dir, custom_client_dir);
        execute(cwd, "npm", &["run", script], None)?;
    }
    if let Some(package) = modern_package {
        validate_generated_client(&root, &client_dir, &package)?;
        eprintln!(
            "nextrs: verified {} through the root workspace link (JavaScript + declarations)",
            package.name
        );
    }
    Ok(())
}

fn prepare_generated_client_for_dev() -> Result<(), String> {
    let root = env::current_dir().map_err(io_error)?;
    let client_package = root.join(DEFAULT_CLIENT_DIR).join("package.json");
    if client_package.is_file() {
        eprintln!("nextrs: refreshing the generated client before starting dev");
        generate_client(GenerateOptions {
            root: PathBuf::from("."),
            client_dir: PathBuf::from(DEFAULT_CLIENT_DIR),
            config: None,
        })?;
    } else if root_declares_generated_client(&root.join("package.json"))? {
        eprintln!("nextrs: materializing and refreshing the generated client before starting dev");
        generate_client(GenerateOptions {
            root: PathBuf::from("."),
            client_dir: PathBuf::from(DEFAULT_CLIENT_DIR),
            config: None,
        })?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClientPackage {
    name: String,
    exports: Vec<ClientExport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClientExport {
    subpath: &'static str,
    types: PathBuf,
    import: PathBuf,
}

fn read_client_package(path: &Path) -> Result<ClientPackage, String> {
    let json = read_json(path)?;
    let name = json
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| format!("{} must contain a non-empty package name", path.display()))?
        .to_string();
    package_install_path(Path::new("node_modules"), &name)?;

    let exports = [".", "./react-query"]
        .into_iter()
        .map(|subpath| {
            let entry = json
                .get("exports")
                .and_then(|exports| exports.get(subpath))
                .ok_or_else(|| {
                    format!(
                        "{} is missing the `{subpath}` package export",
                        path.display()
                    )
                })?;
            Ok(ClientExport {
                subpath,
                types: safe_export_path(path, subpath, entry, "types")?,
                import: safe_export_path(path, subpath, entry, "import")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(ClientPackage { name, exports })
}

fn safe_export_path(
    package_json: &Path,
    subpath: &str,
    entry: &Value,
    condition: &str,
) -> Result<PathBuf, String> {
    let value = entry
        .get(condition)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            format!(
                "{} export `{subpath}` must declare a `{condition}` target",
                package_json.display()
            )
        })?;
    let relative = value.strip_prefix("./").ok_or_else(|| {
        format!(
            "{} export `{subpath}` has unsafe `{condition}` target `{value}`",
            package_json.display()
        )
    })?;
    let path = PathBuf::from(relative);
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        return Err(format!(
            "{} export `{subpath}` has unsafe `{condition}` target `{value}`",
            package_json.display()
        ));
    }
    Ok(path)
}

fn validate_root_client_contract(
    root_package_json: &Path,
    client_name: &str,
) -> Result<(), String> {
    let root = read_json(root_package_json)?;
    let has_workspace = workspace_paths(&root)
        .iter()
        .any(|path| normalized_package_path(path) == DEFAULT_CLIENT_DIR);
    if !has_workspace {
        return Err(format!(
            "{} must list `{DEFAULT_CLIENT_DIR}` in `workspaces` so editors and Node resolve the generated client",
            root_package_json.display()
        ));
    }

    let dependency = ["dependencies", "devDependencies", "optionalDependencies"]
        .into_iter()
        .find_map(|section| {
            root.get(section)
                .and_then(|dependencies| dependencies.get(client_name))
                .and_then(Value::as_str)
        });
    let valid_dependency = dependency
        .and_then(|value| value.strip_prefix("file:"))
        .is_some_and(|path| normalized_package_path(path) == DEFAULT_CLIENT_DIR);
    if !valid_dependency {
        return Err(format!(
            "{} must depend on `{client_name}` via `file:./{DEFAULT_CLIENT_DIR}`",
            root_package_json.display()
        ));
    }
    Ok(())
}

fn root_declares_generated_client(root_package_json: &Path) -> Result<bool, String> {
    if !root_package_json.is_file() {
        return Ok(false);
    }
    let root = read_json(root_package_json)?;
    if workspace_paths(&root)
        .iter()
        .any(|path| normalized_package_path(path) == DEFAULT_CLIENT_DIR)
    {
        return Ok(true);
    }
    Ok(["dependencies", "devDependencies", "optionalDependencies"]
        .into_iter()
        .filter_map(|section| root.get(section).and_then(Value::as_object))
        .flat_map(|dependencies| dependencies.values())
        .filter_map(Value::as_str)
        .filter_map(|value| value.strip_prefix("file:"))
        .any(|path| normalized_package_path(path) == DEFAULT_CLIENT_DIR))
}

fn workspace_paths(root: &Value) -> Vec<&str> {
    let Some(workspaces) = root.get("workspaces") else {
        return Vec::new();
    };
    let packages = workspaces
        .as_array()
        .or_else(|| workspaces.get("packages").and_then(Value::as_array));
    packages
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn normalized_package_path(path: &str) -> &str {
    path.trim().trim_start_matches("./").trim_end_matches('/')
}

fn warn_on_mixed_package_managers(root: &Path) {
    if !root.join("package-lock.json").is_file() {
        return;
    }
    let alternatives = ["pnpm-lock.yaml", "yarn.lock", "bun.lock", "bun.lockb"]
        .into_iter()
        .filter(|lock| root.join(lock).is_file())
        .collect::<Vec<_>>();
    if !alternatives.is_empty() {
        eprintln!(
            "nextrs: warning: found package-lock.json and {}; generated apps use npm, so keep one package manager and remove stale lockfiles",
            alternatives.join(", ")
        );
    }
}

fn ensure_root_client_install(
    root: &Path,
    client_dir: &Path,
    package: &ClientPackage,
) -> Result<(), String> {
    ensure_root_client_install_with(root, client_dir, package, || {
        execute(root, "npm", &["install"], None)
    })
}

fn ensure_root_client_install_with(
    root: &Path,
    client_dir: &Path,
    package: &ClientPackage,
    install: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if client_install_is_valid(root, client_dir, package)? {
        return Ok(());
    }

    eprintln!(
        "nextrs: generated client link is missing or stale; repairing it with a root npm install"
    );
    install()?;
    if client_install_is_valid(root, client_dir, package)? {
        Ok(())
    } else {
        let install_dir = package_install_path(&root.join("node_modules"), &package.name)?;
        Err(format!(
            "npm install completed, but {} is still missing, dangling, or stale. Regenerate `{DEFAULT_CLIENT_DIR}` from the app root and do not install inside it",
            install_dir.display()
        ))
    }
}

fn client_install_is_valid(
    root: &Path,
    client_dir: &Path,
    package: &ClientPackage,
) -> Result<bool, String> {
    let installed_dir = package_install_path(&root.join("node_modules"), &package.name)?;
    let installed_package_json = installed_dir.join("package.json");
    if !installed_package_json.is_file() {
        return Ok(false);
    }
    let source_root = fs::canonicalize(client_dir).map_err(|error| {
        format!(
            "failed to resolve generated client directory {}: {error}",
            client_dir.display()
        )
    })?;
    let Ok(installed_root) = fs::canonicalize(&installed_dir) else {
        return Ok(false);
    };
    if source_root != installed_root {
        return Ok(false);
    }
    let installed = read_client_package(&installed_package_json)?;
    if installed.name != package.name || installed.exports != package.exports {
        return Ok(false);
    }

    // Require the actual workspace link, rather than accepting a copied or
    // cached package with identical metadata that can go stale after codegen.
    let source_manifest = fs::read(client_dir.join("package.json")).map_err(io_error)?;
    let installed_manifest = fs::read(installed_package_json).map_err(io_error)?;
    Ok(source_manifest == installed_manifest)
}

fn validate_generated_client(
    root: &Path,
    client_dir: &Path,
    package: &ClientPackage,
) -> Result<(), String> {
    let installed_dir = package_install_path(&root.join("node_modules"), &package.name)?;
    for export in &package.exports {
        for (condition, relative) in [("types", &export.types), ("import", &export.import)] {
            let source = client_dir.join(relative);
            if !source.is_file() {
                return Err(format!(
                    "generated client export `{}` is missing its {condition} output: {}",
                    export.subpath,
                    source.display()
                ));
            }
            let installed = installed_dir.join(relative);
            if !installed.is_file() {
                return Err(format!(
                    "root package link does not expose the generated {condition} output for `{}`: {}",
                    export.subpath,
                    installed.display()
                ));
            }
        }
    }

    let package_name = serde_json::to_string(&package.name).map_err(|error| error.to_string())?;
    let script =
        format!("await import({package_name}); await import({package_name} + '/react-query')");
    execute(
        root,
        "node",
        &[
            OsStr::new("--input-type=module"),
            OsStr::new("--eval"),
            OsStr::new(&script),
        ],
        None,
    )
    .map_err(|error| {
        format!(
            "generated client files were built, but the consuming app cannot import `{}`: {error}",
            package.name
        )
    })
}

fn package_install_path(node_modules: &Path, package_name: &str) -> Result<PathBuf, String> {
    let parts = package_name.split('/').collect::<Vec<_>>();
    let valid_part =
        |part: &str| !part.is_empty() && part != "." && part != ".." && !part.contains('\\');
    let valid = match parts.as_slice() {
        [name] => !name.starts_with('@') && valid_part(name),
        [scope, name] => {
            scope.starts_with('@') && scope.len() > 1 && valid_part(scope) && valid_part(name)
        }
        _ => false,
    };
    if !valid {
        return Err(format!(
            "invalid generated client package name `{package_name}`"
        ));
    }
    Ok(parts
        .into_iter()
        .fold(node_modules.to_path_buf(), |path, part| path.join(part)))
}

fn read_json(path: &Path) -> Result<Value, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    serde_json::from_str(&contents)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn normal_generation_target<'a>(
    root: &'a Path,
    client_dir: &'a Path,
    custom_client_dir: bool,
) -> (&'a Path, &'static str) {
    if custom_client_dir {
        // Compatibility for applications that explicitly keep a separate
        // generated-client package outside nextrs's hidden default.
        (client_dir, "gen")
    } else {
        (root, "client:generate")
    }
}

fn execute<S: AsRef<OsStr>>(
    cwd: &Path,
    program: &str,
    args: &[S],
    envs: Option<&[(&str, &str)]>,
) -> Result<(), String> {
    let mut command = Command::new(program);
    command.current_dir(cwd).args(args);
    if let Some(envs) = envs {
        command.envs(envs.iter().copied());
    }
    let status = command
        .status()
        .map_err(|error| format!("failed to run `{program}` in {}: {error}", cwd.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{program}` exited with {status}"))
    }
}

fn absolutize(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn io_error(error: std::io::Error) -> String {
    error.to_string()
}

fn print_help() {
    println!(
        "nextrs\n\nUSAGE:\n    nextrs new <PATH> [OPTIONS]\n    nextrs dev [--bin <NAME>] [-- <APP_ARGS>]\n    nextrs client generate [OPTIONS]\n\nRun the same commands as `cargo nextrs ...` or `nextrs ...`.\n\nCLIENT OPTIONS:\n    --root <PATH>        nextrs application root (default: current directory)\n    --client-dir <PATH>  generated package relative to the app root (default: .nextrs/client)\n    --config <PATH>      external-client config; defaults to .nextrs/client/nextrs.client.json when present\n    -h, --help           Print help\n\nClient dependencies are installed once at the application root; never run\n`npm install` inside the generated client directory. Generation validates and\nrepairs the root workspace link, then verifies both JS and declaration exports.\n\nOne `cargo install cargo-nextrs` provides both launchers, the dev server,\nthe legacy `cargo-nextrs-dev` compatibility binary, and client generation."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT_PACKAGE_JSON: &str = r#"{
      "name": "@demo/client",
      "exports": {
        ".": { "types": "./dist/index.d.ts", "import": "./dist/index.js" },
        "./react-query": {
          "types": "./dist/react-query.d.ts",
          "import": "./dist/react-query.js"
        }
      }
    }"#;

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("cargo-nextrs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_modern_package(root: &Path) -> (PathBuf, ClientPackage) {
        let client_dir = root.join(DEFAULT_CLIENT_DIR);
        fs::create_dir_all(&client_dir).unwrap();
        let package_json = client_dir.join("package.json");
        fs::write(&package_json, CLIENT_PACKAGE_JSON).unwrap();
        let package = read_client_package(&package_json).unwrap();
        (client_dir, package)
    }

    fn parse(args: &[&str]) -> Result<CommandLine, String> {
        CommandLine::parse(args.iter().map(OsString::from))
    }

    #[test]
    fn parses_cargo_subcommand_prefix() {
        assert_eq!(
            parse(&["nextrs", "client", "generate"]).unwrap(),
            CommandLine::ClientGenerate(GenerateOptions {
                root: PathBuf::from("."),
                client_dir: PathBuf::from(DEFAULT_CLIENT_DIR),
                config: None,
            })
        );
    }

    #[test]
    fn parses_new_from_both_launchers() {
        let expected = CommandLine::New(vec![
            OsString::from("demo"),
            OsString::from("--nextrs-path"),
            OsString::from("../nextrs"),
        ]);
        assert_eq!(
            parse(&["new", "demo", "--nextrs-path", "../nextrs"]).unwrap(),
            expected
        );
        assert_eq!(
            parse(&["nextrs", "new", "demo", "--nextrs-path", "../nextrs"]).unwrap(),
            expected
        );
    }

    #[test]
    fn parses_generation_paths() {
        assert_eq!(
            parse(&[
                "client",
                "generate",
                "--root",
                "server",
                "--client-dir",
                "web-client",
                "--config",
                "publish.json",
            ])
            .unwrap(),
            CommandLine::ClientGenerate(GenerateOptions {
                root: PathBuf::from("server"),
                client_dir: PathBuf::from("web-client"),
                config: Some(PathBuf::from("publish.json")),
            })
        );
    }

    #[test]
    fn normal_generation_uses_the_root_script() {
        let root = Path::new("/app");
        let generated_client = root.join(DEFAULT_CLIENT_DIR);
        assert_eq!(
            normal_generation_target(root, &generated_client, false),
            (root, "client:generate")
        );
        assert_eq!(
            normal_generation_target(root, Path::new("/custom-client"), true),
            (Path::new("/custom-client"), "gen")
        );
    }

    #[test]
    fn passes_dev_arguments_to_the_dev_runner() {
        assert_eq!(
            parse(&["nextrs", "dev", "--bin", "demo"]).unwrap(),
            CommandLine::Dev(vec![OsString::from("--bin"), OsString::from("demo")])
        );
    }

    #[test]
    fn rejects_unknown_commands() {
        assert!(parse(&["client", "wat"]).is_err());
        assert!(parse(&["wat"]).is_err());
    }

    #[test]
    fn validates_the_root_workspace_and_file_dependency() {
        let root = test_dir("root-contract");
        let package_json = root.join("package.json");
        fs::write(
            &package_json,
            r#"{
              "workspaces": [".nextrs/client"],
              "dependencies": { "@demo/client": "file:./.nextrs/client" }
            }"#,
        )
        .unwrap();
        validate_root_client_contract(&package_json, "@demo/client").unwrap();

        fs::write(
            &package_json,
            r#"{ "dependencies": { "@demo/client": "file:./.nextrs/client" } }"#,
        )
        .unwrap();
        assert!(
            validate_root_client_contract(&package_json, "@demo/client")
                .unwrap_err()
                .contains("workspaces")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn repairs_a_missing_root_client_install_even_when_node_modules_exists() {
        let root = test_dir("repair-link");
        let (client_dir, package) = write_modern_package(&root);
        fs::create_dir_all(root.join("node_modules/unrelated-package")).unwrap();

        let mut installed = false;
        ensure_root_client_install_with(&root, &client_dir, &package, || {
            installed = true;
            let install_dir = package_install_path(&root.join("node_modules"), &package.name)?;
            fs::create_dir_all(install_dir.parent().unwrap()).map_err(io_error)?;
            std::os::unix::fs::symlink(&client_dir, &install_dir).map_err(io_error)?;
            Ok(())
        })
        .unwrap();

        assert!(installed, "the missing client link was not repaired");
        assert!(client_install_is_valid(&root, &client_dir, &package).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn detects_a_declared_client_whose_package_skeleton_is_missing() {
        let root = test_dir("missing-skeleton");
        let package_json = root.join("package.json");
        fs::write(
            &package_json,
            r#"{
              "workspaces": [".nextrs/client"],
              "dependencies": { "@demo/client": "file:./.nextrs/client" }
            }"#,
        )
        .unwrap();
        assert!(root_declares_generated_client(&package_json).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn maps_scoped_package_names_under_root_node_modules() {
        assert_eq!(
            package_install_path(Path::new("/app/node_modules"), "@demo/client").unwrap(),
            Path::new("/app/node_modules/@demo/client")
        );
        assert!(package_install_path(Path::new("node_modules"), "@demo/../client").is_err());
    }
}
