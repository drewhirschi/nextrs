use command_group::{CommandGroup, GroupChild};
#[cfg(unix)]
use command_group::{Signal, UnixChildExt};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify_debouncer_full::notify::event::{AccessKind, AccessMode, MetadataKind, ModifyKind};
use notify_debouncer_full::notify::{EventKind, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, RecommendedCache, new_debouncer};
use serde::Deserialize;
use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError, channel};
use std::thread;
use std::time::{Duration, Instant};

const WATCH_PATHS: &[&str] = &[
    ".cargo/config.toml",
    ".nextrs",
    "Cargo.lock",
    "Cargo.toml",
    "app",
    "build.rs",
    "components",
    "client/package-lock.json",
    "client/package.json",
    "client/src",
    "package-lock.json",
    "package.json",
    "public",
    "src",
    "tsconfig.json",
];

const DEFAULT_IGNORES: &[&str] = &[
    "/target/",
    "/node_modules/",
    "/.nextrs/client/dist/",
    "/.nextrs/client/node_modules/",
    "/.nextrs/client/src/generated/",
    "/.nextrs/openapi.json",
    "/client/node_modules/",
    "/client/src/generated/",
    "/public/dist/",
    ".env",
];

const TERMINATE_GRACE: Duration = Duration::from_secs(1);
const STOP_POLL: Duration = Duration::from_millis(50);

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
    eprintln!("warning: `cargo nextrs-dev` is deprecated; use `cargo nextrs dev` or `nextrs dev`");
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> std::io::Result<()> {
    run_with_args(env::args_os().skip(1))
}

/// Run the dev server with an explicit cargo-subcommand argument list.
///
/// This is public so the unified `cargo-nextrs` package can expose
/// `cargo nextrs dev` while the legacy `cargo-nextrs-dev` binary remains a
/// compatibility entry point for existing `.cargo/config.toml` aliases.
pub fn run_with_args(args: impl IntoIterator<Item = OsString>) -> std::io::Result<()> {
    let mut options = Options::parse(args)?;
    let root = env::current_dir()?;
    let (bin_name, inferred_target_dir) = match options.bin_name.take() {
        Some(bin_name) => (bin_name, None),
        None => {
            let metadata = read_cargo_metadata(&root).map_err(|error| {
                invalid_input(format!(
                    "could not infer app binary: {error}; pass --bin <name>"
                ))
            })?;
            let bin_name = infer_bin_from_metadata(&metadata, &root)?;
            (bin_name, Some(metadata.target_directory))
        }
    };
    let app_path = target_binary(&root, &bin_name, inferred_target_dir.as_deref());
    let ignore_filter = IgnoreFilter::new(&root)?;

    eprintln!("nextrs-dev watching {} paths", WATCH_PATHS.len());
    eprintln!("nextrs-dev build: cargo build --bin {bin_name}");
    eprintln!("nextrs-dev app: {}", app_path.display());

    let shutdown = Arc::new(AtomicBool::new(false));
    let active_app_pgid = Arc::new(AtomicU32::new(0));
    let shutdown_signal = Arc::clone(&shutdown);
    let active_app_signal = Arc::clone(&active_app_pgid);
    ctrlc::set_handler(move || {
        let pgid = active_app_signal.load(Ordering::SeqCst);
        if shutdown_signal.swap(true, Ordering::SeqCst) {
            force_process_group(pgid);
            std::process::exit(130);
        }
        terminate_process_group(pgid);
    })
    .map_err(std::io::Error::other)?;

    let (tx, rx) = channel();
    let mut watcher =
        new_debouncer(Duration::from_secs(1), None, tx).map_err(std::io::Error::other)?;
    watch_paths(&root, &mut watcher)?;

    let mut child = match build_until_current(&root, &bin_name, &rx, &shutdown, &ignore_filter)? {
        BuildOutcome::Ready => Some(spawn_app(&app_path, &options.app_args, &active_app_pgid)?),
        BuildOutcome::Shutdown => return Ok(()),
    };

    loop {
        if shutdown.load(Ordering::SeqCst) {
            if let Some(child) = child.as_mut() {
                eprintln!("nextrs-dev shutting down child");
                stop(child, &active_app_pgid)?;
            }
            return Ok(());
        }

        let child_exited = if let Some(child) = child.as_mut() {
            if let Some(status) = child.try_wait()? {
                eprintln!("nextrs-dev child exited with {status}; waiting for changes");
                cleanup_exited_child(child, &active_app_pgid);
                true
            } else {
                false
            }
        } else {
            false
        };
        if child_exited {
            child = None;
        }

        match recv_change(&rx, &shutdown, &ignore_filter)? {
            Change::Changed => {
                eprintln!("nextrs-dev change detected; rebuilding");
                match build_until_current(&root, &bin_name, &rx, &shutdown, &ignore_filter)? {
                    BuildOutcome::Ready => {
                        if let Some(child) = child.as_mut() {
                            stop(child, &active_app_pgid)?;
                        }
                        child = Some(spawn_app(&app_path, &options.app_args, &active_app_pgid)?);
                    }
                    BuildOutcome::Shutdown => {
                        if let Some(child) = child.as_mut() {
                            stop(child, &active_app_pgid)?;
                        }
                        return Ok(());
                    }
                }
            }
            Change::Shutdown => {
                if let Some(child) = child.as_mut() {
                    stop(child, &active_app_pgid)?;
                }
                return Ok(());
            }
            Change::None => {}
        }
    }
}

struct Options {
    bin_name: Option<String>,
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

        Ok(Self { bin_name, app_args })
    }
}

fn print_help() {
    println!(
        "cargo nextrs-dev\n\nUSAGE:\n    cargo nextrs-dev [--bin <name>] [-- <app-args>...]\n\nBuilds a nextrs app without interrupting in-progress Cargo builds, then runs the built app binary and restarts it after relevant file changes. When --bin is omitted, the current package's default-run or sole binary is used."
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

fn target_binary(root: &Path, bin_name: &str, inferred_target_dir: Option<&Path>) -> PathBuf {
    // CARGO_TARGET_DIR wins if set. Otherwise ask Cargo where the target dir is
    // — for a workspace member (e.g. this repo's `site/`) the build output lands
    // in the *workspace-root* target/, not `<app>/target/`, so a naive
    // `root/target` guess points at a binary that never exists. Fall back to
    // `root/target` only if `cargo metadata` is unavailable.
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .or_else(|| inferred_target_dir.map(Path::to_path_buf))
        .or_else(|| metadata_target_dir(root))
        .unwrap_or_else(|| root.join("target"));
    target_dir
        .join("debug")
        .join(format!("{bin_name}{}", env::consts::EXE_SUFFIX))
}

/// Read `target_directory` from `cargo metadata` (run in `root`). Returns `None`
/// if cargo can't be invoked or the field is missing, so the caller can fall
/// back to a sensible default.
fn metadata_target_dir(root: &Path) -> Option<PathBuf> {
    read_cargo_metadata(root)
        .ok()
        .map(|metadata| metadata.target_directory)
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<MetadataPackage>,
    #[serde(default)]
    workspace_members: Vec<String>,
    #[serde(default)]
    workspace_default_members: Vec<String>,
    target_directory: PathBuf,
}

#[derive(Debug, Deserialize)]
struct MetadataPackage {
    id: String,
    name: String,
    manifest_path: PathBuf,
    #[serde(default)]
    default_run: Option<String>,
    targets: Vec<MetadataTarget>,
}

#[derive(Debug, Deserialize)]
struct MetadataTarget {
    name: String,
    kind: Vec<String>,
}

fn read_cargo_metadata(root: &Path) -> std::io::Result<CargoMetadata> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let output = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        return Err(std::io::Error::other(if detail.is_empty() {
            format!("`cargo metadata` exited with {}", output.status)
        } else {
            format!("`cargo metadata` failed: {detail}")
        }));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| std::io::Error::other(format!("invalid Cargo metadata: {error}")))
}

fn infer_bin_from_metadata(metadata: &CargoMetadata, root: &Path) -> std::io::Result<String> {
    let root = normalize_path(root);
    let current_package = metadata
        .packages
        .iter()
        .filter_map(|package| {
            let package_root = package.manifest_path.parent()?;
            let package_root = normalize_path(package_root);
            root.starts_with(&package_root)
                .then_some((package, package_root.components().count()))
        })
        .max_by_key(|(_, depth)| *depth)
        .map(|(package, _)| package);

    if let Some(package) = current_package {
        return infer_package_bin(package);
    }

    let member_ids = if metadata.workspace_default_members.is_empty() {
        &metadata.workspace_members
    } else {
        &metadata.workspace_default_members
    };
    let packages = metadata
        .packages
        .iter()
        .filter(|package| member_ids.iter().any(|id| id == &package.id))
        .collect::<Vec<_>>();

    if let [package] = packages.as_slice() {
        return infer_package_bin(package);
    }

    let mut bins = packages
        .iter()
        .flat_map(|package| {
            runnable_bins(package)
                .into_iter()
                .map(|bin| format!("{}:{bin}", package.name))
        })
        .collect::<Vec<_>>();
    bins.sort();
    match bins.as_slice() {
        [only] => Ok(only
            .split_once(':')
            .map(|(_, bin)| bin)
            .unwrap_or(only)
            .to_string()),
        [] => Err(invalid_input(
            "could not infer app binary: the current Cargo workspace has no runnable binary; pass --bin <name>",
        )),
        _ => Err(invalid_input(format!(
            "could not infer app binary: the current Cargo workspace has multiple runnable binaries ({}); pass --bin <name>",
            bins.join(", ")
        ))),
    }
}

fn infer_package_bin(package: &MetadataPackage) -> std::io::Result<String> {
    let bins = runnable_bins(package);
    if let Some(default_run) = &package.default_run {
        if bins.iter().any(|bin| bin == default_run) {
            return Ok(default_run.clone());
        }
        return Err(invalid_input(format!(
            "could not infer app binary: package `{}` has default-run `{default_run}`, but no such binary target; pass --bin <name>",
            package.name
        )));
    }

    match bins.as_slice() {
        [only] => Ok((*only).to_string()),
        [] => Err(invalid_input(format!(
            "could not infer app binary: package `{}` has no runnable binary; pass --bin <name>",
            package.name
        ))),
        _ => Err(invalid_input(format!(
            "could not infer app binary: package `{}` has multiple runnable binaries ({}); set package.default-run or pass --bin <name>",
            package.name,
            bins.join(", ")
        ))),
    }
}

fn runnable_bins(package: &MetadataPackage) -> Vec<&str> {
    package
        .targets
        .iter()
        .filter(|target| target.kind.iter().any(|kind| kind == "bin"))
        .map(|target| target.name.as_str())
        .collect()
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
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
                if event_requests_rebuild(&event.kind, &event.paths, ignore_filter) {
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

fn event_requests_rebuild(
    kind: &EventKind,
    paths: &[PathBuf],
    ignore_filter: &IgnoreFilter,
) -> bool {
    should_rebuild(kind) && !event_ignored(paths, ignore_filter)
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

fn spawn_app(
    path: &Path,
    args: &[OsString],
    active_app_pgid: &AtomicU32,
) -> std::io::Result<GroupChild> {
    eprintln!("nextrs-dev starting {}", path.display());
    let mut cmd = Command::new(path);
    cmd.args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let child = cmd.group_spawn()?;
    active_app_pgid.store(child.id(), Ordering::SeqCst);
    Ok(child)
}

fn stop(child: &mut GroupChild, active_app_pgid: &AtomicU32) -> std::io::Result<()> {
    let pgid = child.id();
    terminate_child_group(child);
    if wait_for_child_group(child, TERMINATE_GRACE)? {
        clear_active_app_pgid(active_app_pgid, pgid);
        return Ok(());
    }

    force_child_group(child);
    let _ = child.wait();
    clear_active_app_pgid(active_app_pgid, pgid);
    Ok(())
}

fn cleanup_exited_child(child: &mut GroupChild, active_app_pgid: &AtomicU32) {
    let pgid = child.id();
    terminate_child_group(child);
    let _ = child.wait();
    clear_active_app_pgid(active_app_pgid, pgid);
}

fn wait_for_child_group(child: &mut GroupChild, timeout: Duration) -> std::io::Result<bool> {
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(true);
        }
        if started.elapsed() >= timeout {
            return Ok(false);
        }
        thread::sleep(STOP_POLL);
    }
}

fn clear_active_app_pgid(active_app_pgid: &AtomicU32, pgid: u32) {
    let _ = active_app_pgid.compare_exchange(pgid, 0, Ordering::SeqCst, Ordering::SeqCst);
}

fn terminate_child_group(child: &GroupChild) {
    #[cfg(unix)]
    let _ = child.signal(Signal::SIGTERM);

    #[cfg(not(unix))]
    let _ = child;
}

fn force_child_group(child: &mut GroupChild) {
    let _ = child.kill();
}

fn terminate_process_group(pgid: u32) {
    signal_process_group(pgid, "TERM");
}

fn force_process_group(pgid: u32) {
    signal_process_group(pgid, "KILL");
}

fn signal_process_group(pgid: u32, signal: &str) {
    if pgid == 0 {
        return;
    }

    #[cfg(unix)]
    let _ = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg("--")
        .arg(format!("-{pgid}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    #[cfg(not(unix))]
    let _ = signal;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(
        id: &str,
        name: &str,
        manifest_path: &str,
        bins: &[&str],
        default_run: Option<&str>,
    ) -> MetadataPackage {
        MetadataPackage {
            id: id.to_string(),
            name: name.to_string(),
            manifest_path: PathBuf::from(manifest_path),
            default_run: default_run.map(str::to_string),
            targets: bins
                .iter()
                .map(|name| MetadataTarget {
                    name: (*name).to_string(),
                    kind: vec!["bin".to_string()],
                })
                .collect(),
        }
    }

    fn metadata(
        packages: Vec<MetadataPackage>,
        workspace_members: &[&str],
        default_members: &[&str],
    ) -> CargoMetadata {
        CargoMetadata {
            packages,
            workspace_members: workspace_members
                .iter()
                .map(|id| (*id).to_string())
                .collect(),
            workspace_default_members: default_members.iter().map(|id| (*id).to_string()).collect(),
            target_directory: PathBuf::from("/workspace/target"),
        }
    }

    #[test]
    fn explicit_bin_and_legacy_prefix_are_preserved() {
        let options = Options::parse([
            OsString::from("nextrs-dev"),
            OsString::from("--bin"),
            OsString::from("demo"),
            OsString::from("--"),
            OsString::from("--port"),
            OsString::from("4000"),
        ])
        .unwrap();
        assert_eq!(options.bin_name.as_deref(), Some("demo"));
        assert_eq!(
            options.app_args,
            [OsString::from("--port"), OsString::from("4000")]
        );
    }

    #[test]
    fn omitted_bin_is_deferred_to_metadata_inference() {
        let options = Options::parse(Vec::<OsString>::new()).unwrap();
        assert_eq!(options.bin_name, None);
        assert!(options.app_args.is_empty());
    }

    #[test]
    fn package_default_run_wins_when_multiple_bins_exist() {
        let metadata = metadata(
            vec![package(
                "app-id",
                "app",
                "/workspace/app/Cargo.toml",
                &["app", "index"],
                Some("app"),
            )],
            &["app-id"],
            &["app-id"],
        );
        assert_eq!(
            infer_bin_from_metadata(&metadata, Path::new("/workspace/app/app/todos")).unwrap(),
            "app"
        );
    }

    #[test]
    fn sole_package_binary_is_inferred() {
        let metadata = metadata(
            vec![package(
                "app-id",
                "app",
                "/workspace/app/Cargo.toml",
                &["server"],
                None,
            )],
            &["app-id"],
            &["app-id"],
        );
        assert_eq!(
            infer_bin_from_metadata(&metadata, Path::new("/workspace/app")).unwrap(),
            "server"
        );
    }

    #[test]
    fn sole_workspace_binary_is_inferred_from_virtual_root() {
        let metadata = metadata(
            vec![
                package(
                    "library-id",
                    "library",
                    "/workspace/library/Cargo.toml",
                    &[],
                    None,
                ),
                package(
                    "app-id",
                    "app",
                    "/workspace/app/Cargo.toml",
                    &["server"],
                    None,
                ),
            ],
            &["library-id", "app-id"],
            &[],
        );
        assert_eq!(
            infer_bin_from_metadata(&metadata, Path::new("/workspace")).unwrap(),
            "server"
        );
    }

    #[test]
    fn ambiguous_package_requires_explicit_bin() {
        let metadata = metadata(
            vec![package(
                "app-id",
                "app",
                "/workspace/app/Cargo.toml",
                &["app", "index"],
                None,
            )],
            &["app-id"],
            &["app-id"],
        );
        let error = infer_bin_from_metadata(&metadata, Path::new("/workspace/app")).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("multiple runnable binaries"));
        assert!(error.to_string().contains("--bin <name>"));
    }

    #[test]
    fn watch_paths_cover_modern_and_legacy_project_layouts() {
        for path in [
            ".nextrs",
            "app",
            "components",
            "package-lock.json",
            "package.json",
            "src",
            "tsconfig.json",
        ] {
            assert!(WATCH_PATHS.contains(&path), "missing watch path: {path}");
        }

        // Keep watching the old visible generated-client layout during the
        // migration to `.nextrs/client`.
        for path in [
            "client/package-lock.json",
            "client/package.json",
            "client/src",
        ] {
            assert!(
                WATCH_PATHS.contains(&path),
                "missing legacy watch path: {path}"
            );
        }
    }

    #[test]
    fn generated_outputs_do_not_request_rebuilds() {
        let root = std::env::temp_dir().join(format!(
            "nextrs-dev-generated-output-filter-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let filter = IgnoreFilter::new(&root).unwrap();

        for path in [
            ".nextrs/openapi.json",
            ".nextrs/client/dist/index.js",
            ".nextrs/client/dist/index.d.ts",
            ".nextrs/client/src/generated/fetch/index.ts",
            ".nextrs/client/src/generated/react-query/index.ts",
            "public/dist/app.js",
        ] {
            let path = root.join(path);
            assert!(
                !event_requests_rebuild(&EventKind::Any, &[path], &filter),
                "generated output unexpectedly requests a rebuild"
            );
        }

        for path in [
            "package.json",
            "package-lock.json",
            "components/TodoRow.tsx",
            ".nextrs/dump-openapi.rs",
            ".nextrs/client/package.json",
            ".nextrs/client/orval.config.ts",
            ".nextrs/client/tsconfig.json",
            ".nextrs/client/src/index.ts",
        ] {
            let path = root.join(path);
            assert!(
                event_requests_rebuild(&EventKind::Any, &[path], &filter),
                "source or configuration input unexpectedly ignored"
            );
        }

        std::fs::remove_dir_all(root).unwrap();
    }
}
