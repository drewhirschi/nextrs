use std::collections::BTreeMap;
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
    "Cargo.lock",
    "Cargo.toml",
    "askama.toml",
    "build.rs",
    "nextrs/Cargo.toml",
    "nextrs/src",
    "site/Cargo.toml",
    "site/askama.toml",
    "site/app",
    "site/build.rs",
    "site/content",
    "site/public",
    "site/src",
    "style",
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

    vec!["cargo".into(), "run".into(), "-p".into(), "site".into()]
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
        // dist/ holds build output (page.tsx bundles) — watching it would
        // restart the server after every rebuild that re-bundled.
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
