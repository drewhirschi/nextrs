//! Env-file loading for credential-reading commands (`deploy`, `cron deploy`).
//!
//! The process environment always wins; files only fill in what is unset, so
//! CI secret stores and `CRON_SECRET=… nextrs …` keep working. Target-specific
//! files are searched before the generic `.env`, first hit wins per key:
//!
//! 1. `.vercel/.env.<target>.local` — what `vercel pull` writes
//! 2. `.env.<target>.local`
//! 3. `.env.<target>`
//! 4. `.env.local`
//! 5. `.env`
//!
//! `[deploy] env_file` in `nextrs.toml` replaces that search, and a missing
//! explicit file is an error. Values are never printed — only paths and counts.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct DeployConfig {
    /// One path or a list, relative to the app root. First file wins per key.
    pub env_file: Option<EnvFiles>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum EnvFiles {
    One(PathBuf),
    Many(Vec<PathBuf>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Production,
    Preview,
}

impl Target {
    fn name(self) -> &'static str {
        match self {
            Target::Production => "production",
            Target::Preview => "preview",
        }
    }
}

#[derive(Debug)]
struct Searched {
    path: PathBuf,
    /// `None` when the file does not exist; otherwise the keys it defines.
    keys: Option<Vec<String>>,
}

/// What was searched, for error messages. Holds key names, never values.
#[derive(Debug, Default)]
pub struct Report {
    root: PathBuf,
    searched: Vec<Searched>,
}

impl Report {
    /// Multi-line "searched:" block explaining why `key` was not found.
    pub fn describe_missing(&self, key: &str) -> String {
        if self.searched.is_empty() {
            return String::new();
        }
        let mut out = String::from("\n  searched:");
        for entry in &self.searched {
            let path = entry.path.strip_prefix(&self.root).unwrap_or(&entry.path);
            let note = match &entry.keys {
                None => "not found".to_owned(),
                Some(_) => format!("no {key}"),
            };
            out.push_str(&format!("\n    {} ({note})", path.display()));
        }
        out
    }
}

fn default_candidates(root: &Path, project_dir: Option<&Path>, target: Target) -> Vec<PathBuf> {
    let target = target.name();
    let vercel = format!(".vercel/.env.{target}.local");
    let mut paths = vec![root.join(&vercel)];
    // With a Vercel Root Directory set, `vercel pull` runs (and writes) one
    // level up, in the directory the project root is relative to.
    if let Some(dir) = project_dir.filter(|dir| *dir != root) {
        paths.push(dir.join(&vercel));
    }
    paths.extend([
        root.join(format!(".env.{target}.local")),
        root.join(format!(".env.{target}")),
        root.join(".env.local"),
        root.join(".env"),
    ]);
    paths
}

fn parse(path: &Path) -> Result<Vec<(String, String)>, String> {
    dotenvy::from_path_iter(path)
        .and_then(|iter| iter.collect::<Result<Vec<_>, _>>())
        // dotenvy's parse error echoes the offending line, which may hold a
        // secret — report only the path.
        .map_err(|error| match error {
            dotenvy::Error::LineParse(_, column) => {
                format!("{}: invalid env-file syntax (column {column})", path.display())
            }
            other => format!("{}: {other}", path.display()),
        })
}

/// Resolve the variables the files would supply. Pure: touches no process env.
fn read(
    root: &Path,
    project_dir: Option<&Path>,
    target: Target,
    config: Option<&DeployConfig>,
) -> Result<(BTreeMap<String, String>, Report), String> {
    let explicit = config.and_then(|config| config.env_file.clone());
    let candidates = match &explicit {
        Some(EnvFiles::One(path)) => vec![root.join(path)],
        Some(EnvFiles::Many(paths)) => paths.iter().map(|path| root.join(path)).collect(),
        None => default_candidates(root, project_dir, target),
    };

    let mut vars = BTreeMap::new();
    let mut report = Report {
        root: root.to_path_buf(),
        searched: Vec::new(),
    };
    for path in candidates {
        if !path.is_file() {
            if explicit.is_some() {
                return Err(format!(
                    "[deploy] env_file {} does not exist",
                    path.display()
                ));
            }
            report.searched.push(Searched { path, keys: None });
            continue;
        }
        let pairs = parse(&path)?;
        let keys = pairs.iter().map(|(key, _)| key.clone()).collect();
        for (key, value) in pairs {
            vars.entry(key).or_insert(value);
        }
        report.searched.push(Searched {
            path,
            keys: Some(keys),
        });
    }
    Ok((vars, report))
}

/// Load env files into the process env without overriding anything already
/// set. Call only from the single-threaded CLI path.
pub fn load(
    root: &Path,
    project_dir: Option<&Path>,
    target: Target,
    config: Option<&DeployConfig>,
) -> Result<Report, String> {
    let (vars, report) = read(root, project_dir, target, config)?;
    let mut loaded = 0;
    for (key, value) in vars {
        if std::env::var_os(&key).is_none() {
            // SAFETY: the CLI is single-threaded here; no other thread reads env.
            unsafe { std::env::set_var(&key, value) };
            loaded += 1;
        }
    }
    if loaded > 0 {
        eprintln!("nextrs: loaded {loaded} variables from env files");
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn parses_vercel_quoting() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            ".vercel/.env.production.local",
            "# Created by Vercel CLI\n\nCRON_SECRET=\"abc#def\"\nWITH_EQ=\"a=b=c\"\nMULTI=\"one\\ntwo\"\n",
        );
        let (vars, _) = read(dir.path(), None, Target::Production, None).unwrap();
        assert_eq!(vars["CRON_SECRET"], "abc#def");
        assert_eq!(vars["WITH_EQ"], "a=b=c");
        assert_eq!(vars["MULTI"], "one\ntwo");
    }

    #[test]
    fn target_specific_files_win_over_dotenv() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".env", "CRON_SECRET=generic\nONLY_GENERIC=yes\n");
        write(dir.path(), ".env.production", "CRON_SECRET=prod\n");
        let (vars, _) = read(dir.path(), None, Target::Production, None).unwrap();
        assert_eq!(vars["CRON_SECRET"], "prod");
        assert_eq!(vars["ONLY_GENERIC"], "yes");

        write(
            dir.path(),
            ".vercel/.env.production.local",
            "CRON_SECRET=pulled\n",
        );
        let (vars, _) = read(dir.path(), None, Target::Production, None).unwrap();
        assert_eq!(vars["CRON_SECRET"], "pulled");
    }

    #[test]
    fn preview_never_reads_production_files() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            ".vercel/.env.production.local",
            "CRON_SECRET=prod\n",
        );
        write(dir.path(), ".env.production", "OTHER=prod\n");
        write(dir.path(), ".vercel/.env.preview.local", "CRON_SECRET=pre\n");
        let (vars, _) = read(dir.path(), None, Target::Preview, None).unwrap();
        assert_eq!(vars["CRON_SECRET"], "pre");
        assert!(!vars.contains_key("OTHER"));
    }

    #[test]
    fn reads_pulled_file_from_vercel_project_dir() {
        let repo = tempfile::tempdir().unwrap();
        let root = repo.path().join("site");
        fs::create_dir_all(&root).unwrap();
        write(
            repo.path(),
            ".vercel/.env.production.local",
            "CRON_SECRET=up\n",
        );
        let (vars, _) = read(&root, Some(repo.path()), Target::Production, None).unwrap();
        assert_eq!(vars["CRON_SECRET"], "up");
    }

    #[test]
    fn explicit_env_file_replaces_search_and_must_exist() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".env", "CRON_SECRET=generic\n");
        write(dir.path(), "secrets/deploy.env", "OTHER=1\n");
        let config: DeployConfig = toml::from_str("env_file = \"secrets/deploy.env\"").unwrap();
        let (vars, _) = read(dir.path(), None, Target::Production, Some(&config)).unwrap();
        assert!(!vars.contains_key("CRON_SECRET"));
        assert_eq!(vars["OTHER"], "1");

        let config: DeployConfig =
            toml::from_str("env_file = [\"secrets/deploy.env\", \"nope.env\"]").unwrap();
        let error = read(dir.path(), None, Target::Production, Some(&config)).unwrap_err();
        assert!(error.contains("nope.env"), "{error}");
    }

    #[test]
    fn missing_key_report_lists_paths_but_no_values() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".env", "OTHER=hunter2\n");
        let (_, report) = read(dir.path(), None, Target::Production, None).unwrap();
        let text = report.describe_missing("CRON_SECRET");
        assert!(text.contains(".vercel/.env.production.local (not found)"), "{text}");
        assert!(text.contains(".env.production (not found)"), "{text}");
        assert!(text.contains(".env (no CRON_SECRET)"), "{text}");
        assert!(!text.contains("hunter2"), "{text}");
    }

    #[test]
    fn syntax_errors_do_not_echo_the_line() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".env", "CRON_SECRET='hunter2\n");
        let error = read(dir.path(), None, Target::Production, None).unwrap_err();
        assert!(!error.contains("hunter2"), "{error}");
    }

    #[test]
    fn process_env_wins_over_file() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            ".env",
            "NEXTRS_ENV_FILE_TEST_SET=file\nNEXTRS_ENV_FILE_TEST_UNSET=file\n",
        );
        // SAFETY: keys are unique to this test.
        unsafe { std::env::set_var("NEXTRS_ENV_FILE_TEST_SET", "process") };
        load(dir.path(), None, Target::Production, None).unwrap();
        assert_eq!(std::env::var("NEXTRS_ENV_FILE_TEST_SET").unwrap(), "process");
        assert_eq!(std::env::var("NEXTRS_ENV_FILE_TEST_UNSET").unwrap(), "file");
    }
}
