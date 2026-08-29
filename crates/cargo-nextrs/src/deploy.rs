//! `nextrs deploy` — the whole ship path in one command.
//!
//! 1. `generate` (vercel.json + cron plumbing from nextrs.toml, when present)
//! 2. prebuilt Vercel deploy: `vercel pull` → `vercel build` on this machine
//!    → bundle the compiled function executable into its `.func` dir →
//!    `vercel deploy --prebuilt`. Skips Vercel's build queue entirely.
//! 3. `cron deploy` for cloudflare-provider crons (production only — a
//!    preview URL isn't what the trigger points at).
//!
//! This is the Rust port of `scripts/deploy-prebuilt.sh`; the two must stay
//! behaviorally identical.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::cron;

pub struct DeployOptions {
    pub preview: bool,
    pub skip_cron: bool,
}

pub fn deploy(root: &Path, options: &DeployOptions) -> Result<(), String> {
    let root = fs::canonicalize(root).map_err(|error| format!("bad --root: {error}"))?;

    if root.join(cron::CONFIG_FILE).is_file() {
        let summary = cron::generate(&root)?;
        eprintln!("nextrs: generated {summary}");
        if !options.preview && !options.skip_cron {
            let crons = cron::discover_crons(&root)?;
            let cloudflare: Vec<_> = crons
                .iter()
                .filter(|cron| cron.provider() == cron::Provider::Cloudflare)
                .collect();
            cron::preflight_cloudflare_credentials(&cloudflare)?;
        }
    } else {
        eprintln!(
            "nextrs: no {} — deploying with the existing vercel.json",
            cron::CONFIG_FILE
        );
    }

    let link = root.join(".vercel/project.json");
    let link_json: Value = fs::read_to_string(&link)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or_else(|| {
            format!(
                "{} is not linked to a Vercel project (run `vercel link` in it)",
                root.display()
            )
        })?;

    // Where `vercel build` runs depends on the project's Root Directory
    // setting (the wrong dir silently falls back to static-only output):
    //   - set (monorepo, e.g. "site"): build from the directory the root is
    //     relative to; the CLI descends into it itself.
    //   - unset: the app dir IS the project; build from there, and keep
    //     cargo's target dir inside the upload root so the function's
    //     filePathMap doesn't point outside it.
    let root_directory = link_json["settings"]["rootDirectory"]
        .as_str()
        .filter(|dir| !dir.is_empty())
        .map(str::to_owned);
    let mut envs: Vec<(&str, PathBuf)> = Vec::new();
    let cwd = match &root_directory {
        Some(dir) => {
            let repo = root
                .ancestors()
                .skip(1)
                .find(|anc| fs::canonicalize(anc.join(dir)).ok().as_deref() == Some(&root))
                .ok_or_else(|| {
                    format!(
                        "the Vercel project's Root Directory is `{dir}` but no ancestor of {} contains it at that path",
                        root.display()
                    )
                })?
                .to_path_buf();
            fs::create_dir_all(repo.join(".vercel"))
                .map_err(|error| format!("cannot create {}/.vercel: {error}", repo.display()))?;
            fs::copy(&link, repo.join(".vercel/project.json"))
                .map_err(|error| format!("cannot copy project link: {error}"))?;
            repo
        }
        None => {
            envs.push(("CARGO_TARGET_DIR", root.join("target-vercel")));
            root.clone()
        }
    };

    let environment = if options.preview {
        "preview"
    } else {
        "production"
    };
    let prod: &[&str] = if options.preview { &[] } else { &["--prod"] };

    eprintln!("==> vercel pull (project settings + {environment} env)");
    run(
        &cwd,
        &envs,
        "vercel",
        &["pull", "--yes", &format!("--environment={environment}")],
    )?;

    eprintln!(
        "==> vercel build {} — local compile, incl. the Rust function",
        prod.join(" ")
    );
    run(&cwd, &envs, "vercel", &[&["build"][..], prod].concat())?;

    let functions = cwd.join(".vercel/output/functions");
    let configs = find_files(&functions, ".vc-config.json");
    if configs.is_empty() {
        return Err(
            "no function in .vercel/output — is cargo-zigbuild installed and zig reachable?".into(),
        );
    }
    for config_path in configs {
        bundle_function(&config_path, &cwd)?;
    }

    eprintln!("==> vercel deploy --prebuilt {}", prod.join(" "));
    run(
        &cwd,
        &envs,
        "vercel",
        &[&["deploy", "--prebuilt"][..], prod].concat(),
    )?;

    if options.skip_cron || !root.join(cron::CONFIG_FILE).is_file() {
        return Ok(());
    }
    if options.preview {
        eprintln!(
            "nextrs: preview deploy — skipping cron triggers (they point at the production URL)"
        );
        return Ok(());
    }
    cron::deploy(&root)
}

/// vercel-rust records the compiled executable in `filePathMap`, usually as
/// a path under target/, which is excluded from uploads. Copy it into the
/// `.func` dir and drop the map so the function is self-contained. Relative
/// map entries are relative to the directory `vercel build` ran in.
fn bundle_function(config_path: &Path, build_dir: &Path) -> Result<(), String> {
    let text = fs::read_to_string(config_path)
        .map_err(|error| format!("cannot read {}: {error}", config_path.display()))?;
    let mut config: Value = serde_json::from_str(&text)
        .map_err(|error| format!("{}: {error}", config_path.display()))?;
    let handler = config["handler"]
        .as_str()
        .unwrap_or("executable")
        .to_string();
    let Some(source) = config["filePathMap"][&handler]
        .as_str()
        .map(|source| build_dir.join(source))
    else {
        return Ok(());
    };
    if !source.is_file() {
        return Err(format!(
            "function executable does not exist: {}",
            source.display()
        ));
    }
    let destination = config_path.parent().unwrap().join(&handler);
    fs::copy(&source, &destination).map_err(|error| {
        format!(
            "cannot copy {} → {}: {error}",
            source.display(),
            destination.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&destination)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(perms.mode() | 0o111);
        fs::set_permissions(&destination, perms).map_err(|e| e.to_string())?;
    }
    config.as_object_mut().unwrap().remove("filePathMap");
    fs::write(
        config_path,
        format!("{}\n", serde_json::to_string_pretty(&config).unwrap()),
    )
    .map_err(|error| format!("cannot write {}: {error}", config_path.display()))?;
    eprintln!(
        "==> bundled {} as {}",
        source.display(),
        destination.display()
    );
    Ok(())
}

fn find_files(dir: &Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(find_files(&path, name));
        } else if path.file_name().and_then(|n| n.to_str()) == Some(name) {
            found.push(path);
        }
    }
    found
}

fn run(cwd: &Path, envs: &[(&str, PathBuf)], program: &str, args: &[&str]) -> Result<(), String> {
    let mut command = Command::new(program);
    command.current_dir(cwd).args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    let status = command.status().map_err(|error| {
        format!("failed to run {program} (is it installed? `npm i -g vercel`): {error}")
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {} failed with {status}", args.join(" ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_copies_executable_and_drops_file_path_map() {
        let dir = std::env::temp_dir().join("nextrs-deploy-test-bundle");
        let _ = fs::remove_dir_all(&dir);
        let func = dir.join("api/index.func");
        fs::create_dir_all(&func).unwrap();
        let binary = dir.join("target/release/index");
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::write(&binary, b"#!/bin/sh\n").unwrap();
        let config = func.join(".vc-config.json");
        fs::write(
            &config,
            serde_json::json!({
                "handler": "bootstrap",
                "filePathMap": { "bootstrap": "target/release/index" }
            })
            .to_string(),
        )
        .unwrap();

        bundle_function(&config, &dir).unwrap();

        assert!(func.join("bootstrap").is_file());
        let rewritten: Value = serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
        assert!(rewritten.get("filePathMap").is_none());
        assert_eq!(rewritten["handler"], "bootstrap");
        assert_eq!(find_files(&dir, ".vc-config.json").len(), 1);
    }
}
