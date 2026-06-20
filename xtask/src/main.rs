use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Child, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileState {
    modified: Option<SystemTime>,
    len: u64,
}

type Snapshot = BTreeMap<PathBuf, FileState>;

struct Task {
    watch: bool,
    command: Vec<OsString>,
}

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
    let task = task_from_args();

    if !task.watch {
        eprintln!("cargo dev-once command: {}", display_command(&task.command));
        let mut child = spawn(&task.command)?;
        let status = child.wait()?;
        process::exit(status.code().unwrap_or(1));
    }

    let watch_paths = watch_paths(&root);

    eprintln!("cargo dev watching {} paths", watch_paths.len());
    eprintln!("cargo dev command: {}", display_command(&task.command));

    let mut current = snapshot(&watch_paths)?;
    let mut child = spawn(&task.command)?;

    loop {
        thread::sleep(Duration::from_millis(500));

        if let Some(status) = child.try_wait()? {
            eprintln!("cargo dev child exited with {status}; waiting for changes");
            wait_for_change(&watch_paths, &mut current)?;
            child = spawn(&task.command)?;
            continue;
        }

        let next = snapshot(&watch_paths)?;
        if next != current {
            thread::sleep(Duration::from_millis(150));
            current = snapshot(&watch_paths)?;
            eprintln!("cargo dev change detected; restarting");
            stop(&mut child)?;
            child = spawn(&task.command)?;
        }
    }
}

fn task_from_args() -> Task {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [] => Task {
            watch: true,
            command: default_command(),
        },
        [cmd, rest @ ..] if cmd == OsStr::new("dev") => Task {
            watch: true,
            command: command_from_rest(rest),
        },
        [cmd, rest @ ..] if cmd == OsStr::new("dev-once") => Task {
            watch: false,
            command: command_from_rest(rest),
        },
        [sep, rest @ ..] if sep == OsStr::new("--") => Task {
            watch: true,
            command: command_from_rest(rest),
        },
        [unknown, ..] => {
            eprintln!("usage: cargo dev [-- <command>]");
            eprintln!("   or: cargo dev-once [-- <command>]");
            eprintln!("unknown xtask command: {}", unknown.to_string_lossy());
            process::exit(2);
        }
    }
}

fn command_from_rest(rest: &[OsString]) -> Vec<OsString> {
    let command = if matches!(rest.first(), Some(arg) if arg == OsStr::new("--")) {
        &rest[1..]
    } else {
        rest
    };
    if command.is_empty() {
        default_command()
    } else {
        command.to_vec()
    }
}

fn default_command() -> Vec<OsString> {
    vec!["cargo".into(), "run".into(), "-p".into(), "site".into()]
}

fn watch_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = WATCH_PATHS.iter().map(|rel| root.join(rel)).collect();
    paths.push(env_file_path(root));
    paths
}

fn env_file_path(root: &Path) -> PathBuf {
    let path = env::var_os("NEXTRS_ENV_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".env"));
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn display_command(command: &[OsString]) -> String {
    command
        .iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

fn wait_for_change(paths: &[PathBuf], current: &mut Snapshot) -> std::io::Result<()> {
    loop {
        thread::sleep(Duration::from_millis(500));
        let next = snapshot(paths)?;
        if next != *current {
            *current = next;
            return Ok(());
        }
    }
}

fn snapshot(paths: &[PathBuf]) -> std::io::Result<Snapshot> {
    let mut files = Snapshot::new();
    for path in paths {
        collect_path(path, &mut files)?;
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
        // dist/ holds build output (page.tsx bundles); watching it would
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
        .env("NEXTRS_SKIP_BUNDLE", "0")
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
