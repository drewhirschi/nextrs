use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    match run(env::args_os().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cargo nextrs: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: impl IntoIterator<Item = OsString>) -> Result<(), String> {
    let command = CommandLine::parse(args)?;
    match command {
        CommandLine::Help => {
            print_help();
            Ok(())
        }
        CommandLine::ClientGenerate(options) => generate_client(options),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CommandLine {
    Help,
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
        let mut client_dir = PathBuf::from("client");
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

fn required_path(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<PathBuf, String> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("{flag} requires a path"))
}

fn generate_client(options: GenerateOptions) -> Result<(), String> {
    let root = absolutize(&env::current_dir().map_err(io_error)?, &options.root);
    let client_dir = absolutize(&root, &options.client_dir);
    let package_json = client_dir.join("package.json");
    if !package_json.is_file() {
        return Err(format!(
            "{} does not exist; run this from a nextrs app root or pass --client-dir",
            package_json.display()
        ));
    }

    if !client_dir.join("node_modules").is_dir() {
        eprintln!("cargo nextrs: installing client generator dependencies");
        execute(&client_dir, "npm", &["install"], None)?;
    }

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
            "cargo nextrs: generating internal client and publishing external client from {}",
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
        )
    } else {
        eprintln!("cargo nextrs: generating client from the current Rust contract");
        execute(&client_dir, "npm", &["run", "gen"], None)
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
        "cargo nextrs\n\nUSAGE:\n    cargo nextrs client generate [OPTIONS]\n\nOPTIONS:\n    --root <PATH>        nextrs application root (default: current directory)\n    --client-dir <PATH>  client directory relative to the app root (default: client)\n    --config <PATH>      external-client config; defaults to client/nextrs.client.json when present\n    -h, --help           Print help\n\nThe command installs client dependencies when needed. Without an external config it\nregenerates the app client. With a config it also publishes client.js + client.d.ts."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<CommandLine, String> {
        CommandLine::parse(args.iter().map(OsString::from))
    }

    #[test]
    fn parses_cargo_subcommand_prefix() {
        assert_eq!(
            parse(&["nextrs", "client", "generate"]).unwrap(),
            CommandLine::ClientGenerate(GenerateOptions {
                root: PathBuf::from("."),
                client_dir: PathBuf::from("client"),
                config: None,
            })
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
    fn rejects_unknown_commands() {
        assert!(parse(&["client", "wat"]).is_err());
        assert!(parse(&["wat"]).is_err());
    }
}
