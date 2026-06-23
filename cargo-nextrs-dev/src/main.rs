use command_group::{CommandGroup, GroupChild};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify_debouncer_full::notify::event::{AccessKind, AccessMode, MetadataKind, ModifyKind};
use notify_debouncer_full::notify::{EventKind, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, RecommendedCache, new_debouncer};
use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError, channel};
use std::thread;
use std::time::Duration;

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

const DEFAULT_IGNORES: &[&str] = &[
    "/target/",
    "/node_modules/",
    "/client/node_modules/",
    "/public/dist/",
    ".env",
];

const CARGO_BUILD_ENV: &[&str] = &[
    "CARGO",
    "CARGO_BIN_NAME",
    "CARGO_CRATE_NAME",
    "CARGO_MANIFEST_DIR",
    "CARGO_PKG_AUTHORS",
    "CARGO_PKG_DESCRIPTION",
    "CARGO_PKG_HOMEPAGE",
    "CARGO_PKG_LICENSE",
    "CARGO_PKG_LICENSE_FILE",
    "CARGO_PKG_NAME",
    "CARGO_PKG_README",
    "CARGO_PKG_REPOSITORY",
    "CARGO_PKG_RUST_VERSION",
    "CARGO_PKG_VERSION",
    "CARGO_PKG_VERSION_MAJOR",
    "CARGO_PKG_VERSION_MINOR",
    "CARGO_PKG_VERSION_PATCH",
    "CARGO_PKG_VERSION_PRE",
    "DEBUG",
    "HOST",
    "NUM_JOBS",
    "OPT_LEVEL",
    "OUT_DIR",
    "PROFILE",
    "RUSTC",
    "RUSTDOC",
    "TARGET",
];

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> std::io::Result<()> {
    let options = Options::parse(env::args_os().skip(1))?;
    let root = env::current_dir()?;
    let app_path = target_binary(&root, &options.bin_name);
    let ignore_filter = IgnoreFilter::new(&root)?;

    eprintln!("nextrs-dev watching {} paths", WATCH_PATHS.len());
    eprintln!("nextrs-dev build: cargo build --bin {}", options.bin_name);
    eprintln!("nextrs-dev app: {}", app_path.display());

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_signal = Arc::clone(&shutdown);
    ctrlc::set_handler(move || {
        if shutdown_signal.swap(true, Ordering::SeqCst) {
            std::process::exit(130);
        }
    })
    .map_err(std::io::Error::other)?;

    let (tx, rx) = channel();
    let mut watcher =
        new_debouncer(Duration::from_secs(1), None, tx).map_err(std::io::Error::other)?;
    watch_paths(&root, &mut watcher)?;

    let mut child =
        match build_until_current(&root, &options.bin_name, &rx, &shutdown, &ignore_filter)? {
            BuildOutcome::Ready => Some(spawn_app(&app_path, &options.app_args)?),
            BuildOutcome::Shutdown => return Ok(()),
        };

    loop {
        if shutdown.load(Ordering::SeqCst) {
            if let Some(child) = child.as_mut() {
                eprintln!("nextrs-dev shutting down child");
                stop(child)?;
            }
            return Ok(());
        }

        if let Some(status) = child
            .as_mut()
            .and_then(|child| child.try_wait().transpose())
            .transpose()?
        {
            eprintln!("nextrs-dev child exited with {status}; waiting for changes");
            child = None;
        }

        match recv_change(&rx, &shutdown, &ignore_filter)? {
            Change::Changed => {
                eprintln!("nextrs-dev change detected; rebuilding");
                match build_until_current(&root, &options.bin_name, &rx, &shutdown, &ignore_filter)?
                {
                    BuildOutcome::Ready => {
                        if let Some(child) = child.as_mut() {
                            stop(child)?;
                        }
                        child = Some(spawn_app(&app_path, &options.app_args)?);
                    }
                    BuildOutcome::Shutdown => {
                        if let Some(child) = child.as_mut() {
                            stop(child)?;
                        }
                        return Ok(());
                    }
                }
            }
            Change::Shutdown => {
                if let Some(child) = child.as_mut() {
                    stop(child)?;
                }
                return Ok(());
            }
            Change::None => {}
        }
    }
}

struct Options {
    bin_name: String,
    app_args: Vec<OsString>,
}

impl Options {
    fn parse(args: impl IntoIterator<Item = OsString>) -> std::io::Result<Self> {
        let mut args = args.into_iter().peekable();
        if matches!(args.peek().map(OsString::as_os_str), Some(arg) if arg == "nextrs-dev") {
            args.next();
        }

        let mut bin_name = None;
        let mut app_args = Vec::new();

        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("-h" | "--help") => {
                    print_help();
                    std::process::exit(0);
                }
                Some("--bin") => {
                    let Some(value) = args.next() else {
                        return Err(invalid_input("--bin requires a value"));
                    };
                    bin_name = Some(os_string_to_string("--bin", value)?);
                }
                Some("--") => {
                    app_args.extend(args);
                    break;
                }
                Some(other) if other.starts_with('-') => {
                    return Err(invalid_input(format!("unknown option: {other}")));
                }
                _ => {
                    return Err(invalid_input(format!(
                        "unexpected argument: {}",
                        arg.to_string_lossy()
                    )));
                }
            }
        }

        let Some(bin_name) = bin_name else {
            return Err(invalid_input(
                "missing --bin <name>; generated apps use `cargo dev` to pass this automatically",
            ));
        };

        Ok(Self { bin_name, app_args })
    }
}

fn print_help() {
    println!(
        "cargo nextrs-dev\n\nUSAGE:\n    cargo nextrs-dev --bin <name> [-- <app-args>...]\n\nBuilds a nextrs app without interrupting in-progress Cargo builds, then runs the built app binary and restarts it after relevant file changes."
    );
}

fn invalid_input(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}

fn os_string_to_string(name: &str, value: OsString) -> std::io::Result<String> {
    value
        .into_string()
        .map_err(|value| invalid_input(format!("{name} must be valid UTF-8: {value:?}")))
}

struct IgnoreFilter {
    root: PathBuf,
    matcher: Gitignore,
}

impl IgnoreFilter {
    fn new(root: &Path) -> std::io::Result<Self> {
        let mut builder = GitignoreBuilder::new(root);

        for name in [".gitignore", ".ignore"] {
            let path = root.join(name);
            if path.is_file()
                && let Some(err) = builder.add(&path)
            {
                eprintln!("nextrs-dev ignore warning: {err}");
            }
        }

        for pattern in DEFAULT_IGNORES {
            builder
                .add_line(None, pattern)
                .map_err(std::io::Error::other)?;
        }

        Ok(Self {
            root: root.to_path_buf(),
            matcher: builder.build().map_err(std::io::Error::other)?,
        })
    }

    fn is_ignored(&self, path: &Path) -> bool {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };

        if !path.starts_with(&self.root) {
            return false;
        }

        self.matcher
            .matched_path_or_any_parents(&path, false)
            .is_ignore()
            || self
                .matcher
                .matched_path_or_any_parents(&path, true)
                .is_ignore()
    }
}

fn target_binary(root: &Path, bin_name: &str) -> PathBuf {
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    target_dir
        .join("debug")
        .join(format!("{bin_name}{}", env::consts::EXE_SUFFIX))
}

fn watch_paths(
    root: &Path,
    watcher: &mut notify_debouncer_full::Debouncer<
        notify_debouncer_full::notify::RecommendedWatcher,
        RecommendedCache,
    >,
) -> std::io::Result<()> {
    for rel in WATCH_PATHS {
        let path = root.join(rel);
        if path.exists() {
            watcher
                .watch(&path, RecursiveMode::Recursive)
                .map_err(std::io::Error::other)?;
        }
    }
    Ok(())
}

enum Change {
    Changed,
    None,
    Shutdown,
}

fn recv_change(
    rx: &Receiver<DebounceEventResult>,
    shutdown: &AtomicBool,
    ignore_filter: &IgnoreFilter,
) -> std::io::Result<Change> {
    if shutdown.load(Ordering::SeqCst) {
        return Ok(Change::Shutdown);
    }

    match rx.recv_timeout(Duration::from_millis(250)) {
        Ok(result) => {
            let changed =
                log_watch_result(result, ignore_filter)? | drain_changes(rx, ignore_filter)?;
            if changed {
                Ok(Change::Changed)
            } else {
                Ok(Change::None)
            }
        }
        Err(RecvTimeoutError::Timeout) => {
            if shutdown.load(Ordering::SeqCst) {
                Ok(Change::Shutdown)
            } else {
                Ok(Change::None)
            }
        }
        Err(RecvTimeoutError::Disconnected) => Err(std::io::Error::other("file watcher stopped")),
    }
}

fn wait_for_change(
    rx: &Receiver<DebounceEventResult>,
    shutdown: &AtomicBool,
    ignore_filter: &IgnoreFilter,
) -> std::io::Result<Change> {
    loop {
        match recv_change(rx, shutdown, ignore_filter)? {
            Change::None => continue,
            other => return Ok(other),
        }
    }
}

fn drain_changes(
    rx: &Receiver<DebounceEventResult>,
    ignore_filter: &IgnoreFilter,
) -> std::io::Result<bool> {
    let mut changed = false;
    loop {
        match rx.try_recv() {
            Ok(result) => {
                changed |= log_watch_result(result, ignore_filter)?;
            }
            Err(TryRecvError::Empty) => return Ok(changed),
            Err(TryRecvError::Disconnected) => {
                return Err(std::io::Error::other("file watcher stopped"));
            }
        }
    }
}

fn log_watch_result(
    result: DebounceEventResult,
    ignore_filter: &IgnoreFilter,
) -> std::io::Result<bool> {
    match result {
        Ok(events) => {
            let mut changed = 0usize;
            for event in events {
                if should_rebuild(&event.kind) && !event_ignored(&event.paths, ignore_filter) {
                    changed += 1;
                    if changed <= 8 {
                        eprintln!("nextrs-dev changed: {}", event_paths(&event.paths));
                    }
                }
            }
            if changed > 8 {
                eprintln!("nextrs-dev changed: ... and {} more events", changed - 8);
            }
            Ok(changed > 0)
        }
        Err(errors) => {
            for error in &errors {
                eprintln!("nextrs-dev watcher error: {error}");
            }
            Err(std::io::Error::other("file watcher error"))
        }
    }
}

fn event_ignored(paths: &[PathBuf], ignore_filter: &IgnoreFilter) -> bool {
    !paths.is_empty() && paths.iter().all(|path| ignore_filter.is_ignored(path))
}

fn should_rebuild(kind: &EventKind) -> bool {
    match kind {
        EventKind::Create(_) | EventKind::Remove(_) => true,
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime)) => false,
        EventKind::Modify(_) => true,
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        EventKind::Any | EventKind::Other => true,
    }
}

fn event_paths(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return "<unknown>".to_string();
    }

    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(" -> ")
}

enum BuildOutcome {
    Ready,
    Shutdown,
}

fn build_until_current(
    root: &Path,
    bin_name: &str,
    rx: &Receiver<DebounceEventResult>,
    shutdown: &AtomicBool,
    ignore_filter: &IgnoreFilter,
) -> std::io::Result<BuildOutcome> {
    loop {
        match run_build(root, bin_name, shutdown)? {
            BuildRun::Success => {
                if drain_changes(rx, ignore_filter)? {
                    eprintln!("nextrs-dev changes arrived during build; rebuilding once more");
                    continue;
                }
                return Ok(BuildOutcome::Ready);
            }
            BuildRun::Failed(status) => {
                eprintln!("nextrs-dev build failed with {status}; waiting for changes");
                match wait_for_change(rx, shutdown, ignore_filter)? {
                    Change::Changed => continue,
                    Change::Shutdown => return Ok(BuildOutcome::Shutdown),
                    Change::None => {}
                }
            }
            BuildRun::Shutdown => return Ok(BuildOutcome::Shutdown),
        }
    }
}

enum BuildRun {
    Success,
    Failed(ExitStatus),
    Shutdown,
}

fn run_build(root: &Path, bin_name: &str, shutdown: &AtomicBool) -> std::io::Result<BuildRun> {
    eprintln!("nextrs-dev building {bin_name}");
    let mut command = Command::new("cargo");
    command
        .arg("build")
        .arg("--bin")
        .arg(bin_name)
        .current_dir(root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    scrub_cargo_build_env(&mut command);

    let mut child = command.group_spawn()?;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(BuildRun::Shutdown);
        }

        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(BuildRun::Success)
            } else {
                Ok(BuildRun::Failed(status))
            };
        }

        thread::sleep(Duration::from_millis(250));
    }
}

fn scrub_cargo_build_env(command: &mut Command) {
    for key in CARGO_BUILD_ENV {
        command.env_remove(key);
    }
    for (key, _) in env::vars_os() {
        if key_string_starts_with(&key, "DEP_") {
            command.env_remove(key);
        }
    }
}

fn key_string_starts_with(key: &OsStr, prefix: &str) -> bool {
    key.to_str().is_some_and(|key| key.starts_with(prefix))
}

fn spawn_app(path: &Path, args: &[OsString]) -> std::io::Result<GroupChild> {
    eprintln!("nextrs-dev starting {}", path.display());
    let mut cmd = Command::new(path);
    cmd.args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    cmd.group_spawn()
}

fn stop(child: &mut GroupChild) -> std::io::Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}
