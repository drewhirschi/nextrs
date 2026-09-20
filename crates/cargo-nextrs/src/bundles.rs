//! Compile isolated server artifacts and their platform routing from one plan.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use nextrs::server_bundles::BundlePlan;
use serde_json::{Value, json};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct BuildOptions {
    pub bin: Option<String>,
    pub vercel: bool,
    pub dev: bool,
    /// `--output`: where the finished bundle directory lands.
    pub output: Option<PathBuf>,
}

pub fn plan(root: &Path) -> Result<BundlePlan, String> {
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let app = root.join("app");
    let routes = nextrs::discovery::discover_routes(&app);
    let plan = BundlePlan::discover(&root, &app, &routes)?;
    // Fail ambiguous patterns before separate routers could hide a collision.
    let mut patterns = std::collections::BTreeMap::new();
    for path in plan.bundles.values().flat_map(|bundle| &bundle.routes) {
        if path == "/__nextrs_functions" || path.starts_with("/__nextrs_functions/") {
            return Err(format!(
                "{path}: /__nextrs_functions is reserved for deployment"
            ));
        }
        if let Some(previous) = patterns.insert(route_regex(path), path) {
            return Err(format!(
                "ambiguous server routes {previous} and {path} match the same URL pattern"
            ));
        }
    }
    // Validate before any expensive compilation, including deferred prefetch.
    for name in plan.bundles.keys() {
        plan.select(&routes, name)?;
    }
    Ok(plan)
}

pub fn explain(root: &Path) -> Result<(), String> {
    let plan = plan(root)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&plan).map_err(|e| e.to_string())?
    );
    Ok(())
}

/// Escape literal path components; dynamic segments follow Axum's specificity.
fn route_regex(path: &str) -> String {
    if path == "/" {
        return "^/$".into();
    }
    let mut result = String::from("^");
    for segment in path.trim_start_matches('/').split('/') {
        result.push('/');
        if segment.starts_with("{*") && segment.ends_with('}') {
            result.push_str(".+");
        } else if segment.starts_with('{') && segment.ends_with('}') {
            result.push_str("[^/]+");
        } else {
            for c in segment.chars() {
                if ".+*?()[]{}^$|\\".contains(c) {
                    result.push('\\');
                }
                result.push(c);
            }
        }
    }
    result.push('$');
    result
}

fn specificity(path: &str) -> Vec<(u8, &str)> {
    path.split('/')
        .map(|s| {
            (
                if s.starts_with("{*") {
                    2
                } else if s.starts_with('{') {
                    1
                } else {
                    0
                },
                s,
            )
        })
        .collect()
}

pub fn routing(plan: &BundlePlan) -> Vec<Value> {
    let mut paths: Vec<_> = plan
        .bundles
        .values()
        .flat_map(|b| b.routes.iter().map(move |p| (p, &b.name)))
        .collect();
    paths.sort_by(|(a, _), (b, _)| specificity(a).cmp(&specificity(b)));
    // Function entry URLs are internal. Original public URLs are matched below.
    let mut rules = vec![
        json!({"src":"^/__nextrs_functions(?:/.*)?$", "status":404}),
        json!({"src":"^/dist/.*$", "headers":{"Cache-Control":"public, max-age=31536000, immutable"}, "continue":true}),
        json!({"handle":"filesystem"}),
    ];
    for (path, name) in paths {
        // Route all methods to the owner, letting Axum implement HEAD/OPTIONS/405.
        // Public path and query are retained by Vercel's executable runtime.
        rules.push(json!({"src":route_regex(path), "dest":format!("/__nextrs_functions/{name}")}));
    }
    rules.push(json!({"src":"^/.*$", "dest":"/__nextrs_functions/default"}));
    rules
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(source).map_err(|e| format!("{}: {e}", source.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "asset {} is a symlink; use a regular file or directory",
            source.display()
        ));
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if metadata.is_file() {
        fs::create_dir_all(destination.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::copy(source, destination).map_err(|e| e.to_string())?;
    } else {
        return Err(format!("{} is not a regular asset", source.display()));
    }
    Ok(())
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    fs::write(
        path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
        ),
    )
    .map_err(|e| format!("{}: {e}", path.display()))
}

pub fn build(
    root: &Path,
    options: &BuildOptions,
    output: Option<&Path>,
) -> Result<PathBuf, String> {
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let plan = plan(&root)?;
    if options.vercel && options.dev {
        return Err("--dev cannot be used with --vercel".into());
    }
    let metadata = Command::new("cargo")
        .current_dir(&root)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .map_err(|e| e.to_string())?;
    if !metadata.status.success() {
        return Err(String::from_utf8_lossy(&metadata.stderr).into());
    }
    let metadata: Value = serde_json::from_slice(&metadata.stdout).map_err(|e| e.to_string())?;
    let manifest = root.join("Cargo.toml");
    let package = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["manifest_path"].as_str() == manifest.to_str())
        .ok_or("--root must select an application package, not a virtual workspace")?;
    let bin = options
        .bin
        .as_deref()
        .or(if options.vercel {
            Some("index")
        } else {
            package["default_run"].as_str().or(package["name"].as_str())
        })
        .unwrap();
    if !package["targets"].as_array().unwrap().iter().any(|t| {
        t["name"] == bin
            && t["kind"]
                .as_array()
                .is_some_and(|k| k.contains(&json!("bin")))
    }) {
        return Err(format!("package {} has no binary {bin:?}", package["name"]));
    }
    let output = output.map(Path::to_path_buf).unwrap_or_else(|| {
        root.join(if options.vercel {
            ".vercel/output"
        } else {
            ".nextrs/bundles"
        })
    });
    fs::create_dir_all(root.join(".nextrs")).map_err(|e| e.to_string())?;
    let stage = root.join(format!(".nextrs/bundle-build-{}", std::process::id()));
    fs::create_dir(&stage).map_err(|e| format!("{}: {e}", stage.display()))?;
    let result = build_into(
        &root,
        options,
        &plan,
        bin,
        package["id"].as_str().unwrap(),
        &stage,
    );
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }
    fs::create_dir_all(output.parent().unwrap()).map_err(|e| e.to_string())?;
    if output.exists() {
        fs::remove_dir_all(&output).map_err(|e| e.to_string())?;
    }
    // `--output` may sit on another filesystem (a mounted volume, a tmpfs),
    // where rename fails with EXDEV; fall back to copying.
    if fs::rename(&stage, &output).is_err() {
        let copied = copy_tree(&stage, &output);
        let _ = fs::remove_dir_all(&stage);
        copied?;
    }
    eprintln!(
        "nextrs: wrote {} server bundle(s) to {}",
        plan.bundles.len(),
        output.display()
    );
    Ok(output)
}

fn build_into(
    root: &Path,
    options: &BuildOptions,
    plan: &BundlePlan,
    bin: &str,
    package_id: &str,
    stage: &Path,
) -> Result<(), String> {
    let mut artifacts = serde_json::Map::new();
    for bundle in plan.bundles.values() {
        eprintln!(
            "nextrs: building {} (features: {})",
            bundle.name,
            bundle.features.join(", ")
        );
        let mut command = Command::new("cargo");
        command
            .current_dir(root)
            .env("NEXTRS_SERVER_BUNDLE", &bundle.name)
            .arg(if options.vercel { "zigbuild" } else { "build" })
            .args([
                "--locked",
                "--bin",
                bin,
                "--no-default-features",
                "--message-format=json-render-diagnostics",
            ])
            .stderr(Stdio::inherit());
        if !options.dev {
            command.arg("--release");
        }
        if options.vercel {
            command.args(["--target", "x86_64-unknown-linux-gnu.2.26"]);
        }
        if !bundle.features.is_empty() {
            command.arg("--features").arg(bundle.features.join(","));
        }
        let output = command.output().map_err(|e| format!("cargo: {e}"))?;
        if !output.status.success() {
            return Err(format!("server bundle {} failed to compile", bundle.name));
        }
        let receipt_path = String::from_utf8_lossy(&output.stdout).lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|event| event["reason"] == "build-script-executed" && event["package_id"] == package_id)
            .and_then(|event| event["out_dir"].as_str().map(|dir| Path::new(dir).join("nextrs-server-bundle.json")))
            .ok_or("application build script did not report server bundle selection; update nextrs and call emit_registry from build.rs")?;
        let receipt: Value = fs::read(&receipt_path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or("missing server bundle receipt; update the application's nextrs dependency to a version supporting server bundles")?;
        if receipt["bundle"] != bundle.name || receipt["routes"] != json!(bundle.routes) {
            return Err(format!(
                "server bundle {} codegen disagrees with deployment plan; refusing to package it",
                bundle.name
            ));
        }
        let executable = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|event| {
                event["reason"] == "compiler-artifact" && event["target"]["name"] == bin
            })
            .filter_map(|event| event["executable"].as_str().map(PathBuf::from))
            .last()
            .ok_or_else(|| format!("cargo returned no executable for {}", bundle.name))?;
        let directory = if options.vercel {
            function_dir(stage, &bundle.name)
        } else {
            stage.join(&bundle.name)
        };
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        fs::copy(&executable, directory.join("executable")).map_err(|e| e.to_string())?;
        for asset in &bundle.assets {
            if matches!(
                asset
                    .components()
                    .next()
                    .and_then(|p| p.as_os_str().to_str()),
                Some(
                    "executable"
                        | ".vc-config.json"
                        | ".nextrs"
                        | ".vercel"
                        | "target"
                        | "target-vercel"
                )
            ) {
                return Err(format!("reserved bundle asset path {}", asset.display()));
            }
            // Check intermediate directories too; a file reached through a
            // symlink must not escape the declared application resource tree.
            let mut source = root.to_path_buf();
            for component in asset.components() {
                source.push(component);
                if fs::symlink_metadata(&source)
                    .map_err(|e| format!("{}: {e}", source.display()))?
                    .file_type()
                    .is_symlink()
                {
                    return Err(format!("asset {} traverses a symlink", asset.display()));
                }
            }
            copy_tree(&source, &directory.join(asset))?;
        }
        artifacts.insert(bundle.name.clone(), json!({"executable_bytes":fs::metadata(&executable).map_err(|e| e.to_string())?.len(), "features":bundle.features, "assets":bundle.assets}));
    }
    write_json(&stage.join("bundle-artifacts.json"), &artifacts)?;
    if options.vercel && root.join("public").is_dir() {
        copy_tree(&root.join("public"), &stage.join("static"))?;
    }
    write_plan_metadata(root, plan, stage, options.vercel)
}

/// Everything in the output that derives from the plan and `nextrs.toml`
/// rather than from compilation: manifest, routing, and (for Vercel) each
/// function's `.vc-config.json` plus the root `config.json`.
fn write_plan_metadata(
    root: &Path,
    plan: &BundlePlan,
    output: &Path,
    vercel: bool,
) -> Result<(), String> {
    write_json(&output.join("bundle-manifest.json"), plan)?;
    write_json(&output.join("routing.json"), &routing(plan))?;
    if !vercel {
        return Ok(());
    }
    // Same native executable runtime contract emitted by vercel-rust.
    let settings = crate::cron::load_config(root)?.vercel.unwrap_or_default();
    let mut config = json!({"handler":"executable","runtime":"executable","runtimeLanguage":"rust","architecture":"x86_64","environment":{},"supportsResponseStreaming":true});
    if !settings.regions.is_empty() {
        config["regions"] = json!(settings.regions);
    }
    for bundle in plan.bundles.values() {
        write_json(
            &function_dir(output, &bundle.name).join(".vc-config.json"),
            &config,
        )?;
    }
    let crons: Vec<_> = crate::cron::discover_crons(root)?
        .into_iter()
        .filter(|c| c.provider() == crate::cron::Provider::Vercel)
        .map(|c| json!({"path":c.path,"schedule":c.schedule}))
        .collect();
    write_json(
        &output.join("config.json"),
        &json!({"version":3,"routes":routing(plan),"crons":crons}),
    )
}

fn function_dir(output: &Path, bundle: &str) -> PathBuf {
    output.join(format!("functions/__nextrs_functions/{bundle}.func"))
}

/// Fail before an expensive build when a bundle declares an asset that is not
/// on disk (e.g. a gitignored private library missing from a fresh clone).
pub fn check_declared_assets(root: &Path, plan: &BundlePlan) -> Result<(), String> {
    for bundle in plan.bundles.values() {
        for asset in &bundle.assets {
            if !root.join(asset).exists() {
                return Err(format!(
                    "bundle {} declares asset {} but {} does not exist",
                    bundle.name,
                    asset.display(),
                    root.join(asset).display()
                ));
            }
        }
    }
    Ok(())
}

/// Run `[build] command` in place of the in-process compile, then hold its
/// output to the packaging rules. nextrs did not perform this build, so it
/// trusts nothing about it beyond what [`verify_vercel_output`] checks.
pub fn build_with_command(root: &Path, command: &str, output: &Path) -> Result<(), String> {
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let plan = plan(&root)?;
    // A stale output from an earlier run must never satisfy the rules.
    if output.exists() {
        fs::remove_dir_all(output).map_err(|e| format!("{}: {e}", output.display()))?;
    }
    eprintln!("==> [build] command: {command}");
    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(&root)
        .env("NEXTRS_BUNDLE_OUTPUT", output)
        .env("NEXTRS_BUILD_TARGET", "vercel")
        // The host already built the frontend; the app's build.rs reuses it.
        .env("NEXTRS_SKIP_BUNDLE", "1")
        .status()
        .map_err(|e| format!("failed to run [build] command `{command}`: {e}"))?;
    if !status.success() {
        return Err(format!("[build] command `{command}` failed with {status}"));
    }
    verify_vercel_output(&plan, output)
        .map_err(|rule| format!("[build] command `{command}` broke a packaging rule: {rule}\n  See https://nextrs.hirschi.dev/docs/custom-build"))?;
    // Plan-derived files stay framework-owned whatever the command wrote.
    write_plan_metadata(&root, &plan, output, true)?;
    if !output.join("static").is_dir() && root.join("public").is_dir() {
        copy_tree(&root.join("public"), &output.join("static"))?;
    }
    eprintln!(
        "nextrs: verified {} server bundle(s) in {}",
        plan.bundles.len(),
        output.display()
    );
    Ok(())
}

/// `nextrs bundles verify`: check an existing output against the packaging
/// rules without deploying — the loop for authoring a `[build] command`.
pub fn verify(root: &Path, output: Option<&Path>) -> Result<(), String> {
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let plan = plan(&root)?;
    let output = output
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join(".vercel/output"));
    verify_vercel_output(&plan, &output)?;
    eprintln!(
        "nextrs: {} follows the packaging rules ({} bundle(s))",
        output.display(),
        plan.bundles.len()
    );
    Ok(())
}

/// The packaging rules for a custom-built Vercel output.
fn verify_vercel_output(plan: &BundlePlan, output: &Path) -> Result<(), String> {
    let manifest_path = output.join("bundle-manifest.json");
    let manifest: Value = fs::read(&manifest_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or_else(|| {
            format!(
                "{} is missing or unreadable (write the output to $NEXTRS_BUNDLE_OUTPUT, e.g. `nextrs bundles build --vercel --output \"$NEXTRS_BUNDLE_OUTPUT\"`)",
                manifest_path.display()
            )
        })?;
    if manifest != serde_json::to_value(plan).map_err(|e| e.to_string())? {
        return Err(format!(
            "{} does not match this app's bundle plan; the build ran against different sources or config",
            manifest_path.display()
        ));
    }
    for bundle in plan.bundles.values() {
        let directory = function_dir(output, &bundle.name);
        let executable = directory.join("executable");
        let bytes = fs::read(&executable).map_err(|_| {
            format!(
                "bundle {} has no executable at {}",
                bundle.name,
                executable.display()
            )
        })?;
        // ELF magic, then e_machine (little-endian u16 at 18) == EM_X86_64.
        if bytes.len() < 20 || &bytes[..4] != b"\x7fELF" || bytes[18..20] != [62, 0] {
            return Err(format!(
                "{} is not an x86-64 Linux (ELF) executable, which is what Vercel runs",
                executable.display()
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&executable)
                .map_err(|e| e.to_string())?
                .permissions()
                .mode();
            if mode & 0o111 == 0 {
                return Err(format!("{} is not marked executable", executable.display()));
            }
        }
        for asset in &bundle.assets {
            if !directory.join(asset).exists() {
                return Err(format!(
                    "bundle {} declares asset {} but it is not in {}",
                    bundle.name,
                    asset.display(),
                    directory.display()
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routes_preserve_specificity_and_all_methods() {
        let plan: BundlePlan = serde_json::from_value(json!({"version":1,"owners":{},"bundles":{
            "default":{"name":"default","routes":["/api/items/new", "/api/literal.+"],"features":[],"assets":[]},
            "heavy":{"name":"heavy","routes":["/api/items/{id}","/api/{*rest}"],"features":[],"assets":[]}
        }})).unwrap();
        let rules = routing(&plan);
        let dispatch = |path: &str| {
            rules
                .iter()
                .filter(|r| r.get("dest").is_some())
                .find(|r| {
                    regex::Regex::new(r["src"].as_str().unwrap())
                        .unwrap()
                        .is_match(path)
                })
                .unwrap()["dest"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(dispatch("/api/items/new"), "/__nextrs_functions/default");
        assert_eq!(dispatch("/api/items/42"), "/__nextrs_functions/heavy");
        assert_eq!(dispatch("/api/a/b"), "/__nextrs_functions/heavy");
        assert_eq!(dispatch("/api/literal.+"), "/__nextrs_functions/default");
        assert_eq!(dispatch("/api/literalZZ"), "/__nextrs_functions/heavy");
        assert!(rules.iter().all(|r| r.get("methods").is_none()));
        assert!(
            !regex::Regex::new(&route_regex("/api/{*rest}"))
                .unwrap()
                .is_match("/api/")
        );
    }
    /// 20 bytes of ELF header: magic, then e_machine at offset 18.
    fn elf(machine: u8) -> Vec<u8> {
        let mut bytes = b"\x7fELF".to_vec();
        bytes.resize(18, 0);
        bytes.extend([machine, 0]);
        bytes
    }

    fn packaged(plan: &BundlePlan, output: &Path) {
        write_json(&output.join("bundle-manifest.json"), plan).unwrap();
        for bundle in plan.bundles.values() {
            let directory = function_dir(output, &bundle.name);
            fs::create_dir_all(&directory).unwrap();
            let executable = directory.join("executable");
            fs::write(&executable, elf(62)).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
            }
            for asset in &bundle.assets {
                fs::create_dir_all(directory.join(asset)).unwrap();
            }
        }
    }

    fn vision_plan() -> BundlePlan {
        serde_json::from_value(json!({"version":1,"owners":{},"bundles":{
            "default":{"name":"default","routes":["/"],"features":[],"assets":[]},
            "vision":{"name":"vision","routes":["/api/faces"],"features":["face-inference"],"assets":["resources/vision"]}
        }}))
        .unwrap()
    }

    #[test]
    fn custom_output_passes_when_it_follows_the_rules() {
        let temp = tempfile::tempdir().unwrap();
        packaged(&vision_plan(), temp.path());
        verify_vercel_output(&vision_plan(), temp.path()).unwrap();
    }

    #[test]
    fn custom_output_rules_each_fail_with_a_named_reason() {
        let plan = vision_plan();
        let func = |root: &Path| function_dir(root, "vision");

        let temp = tempfile::tempdir().unwrap();
        let error = verify_vercel_output(&plan, temp.path()).unwrap_err();
        assert!(error.contains("NEXTRS_BUNDLE_OUTPUT"), "{error}");

        let temp = tempfile::tempdir().unwrap();
        packaged(&plan, temp.path());
        let mut stale = plan.clone();
        stale.bundles.get_mut("vision").unwrap().features.clear();
        write_json(&temp.path().join("bundle-manifest.json"), &stale).unwrap();
        let error = verify_vercel_output(&plan, temp.path()).unwrap_err();
        assert!(error.contains("does not match this app's bundle plan"), "{error}");

        let temp = tempfile::tempdir().unwrap();
        packaged(&plan, temp.path());
        fs::remove_file(func(temp.path()).join("executable")).unwrap();
        let error = verify_vercel_output(&plan, temp.path()).unwrap_err();
        assert!(error.contains("bundle vision has no executable"), "{error}");

        // aarch64 (183) — e.g. built natively on an ARM Mac.
        let temp = tempfile::tempdir().unwrap();
        packaged(&plan, temp.path());
        fs::write(func(temp.path()).join("executable"), elf(183)).unwrap();
        let error = verify_vercel_output(&plan, temp.path()).unwrap_err();
        assert!(error.contains("not an x86-64 Linux"), "{error}");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let temp = tempfile::tempdir().unwrap();
            packaged(&plan, temp.path());
            let executable = func(temp.path()).join("executable");
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o644)).unwrap();
            let error = verify_vercel_output(&plan, temp.path()).unwrap_err();
            assert!(error.contains("not marked executable"), "{error}");
        }

        let temp = tempfile::tempdir().unwrap();
        packaged(&plan, temp.path());
        fs::remove_dir_all(func(temp.path()).join("resources")).unwrap();
        let error = verify_vercel_output(&plan, temp.path()).unwrap_err();
        assert!(error.contains("declares asset resources/vision"), "{error}");
    }

    #[test]
    fn missing_declared_asset_fails_before_building() {
        let temp = tempfile::tempdir().unwrap();
        let error = check_declared_assets(temp.path(), &vision_plan()).unwrap_err();
        assert!(error.contains("bundle vision declares asset resources/vision"), "{error}");
        fs::create_dir_all(temp.path().join("resources/vision")).unwrap();
        check_declared_assets(temp.path(), &vision_plan()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn build_command_runs_then_nextrs_owns_the_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("app-root");
        fs::create_dir_all(root.join("app/api/ping")).unwrap();
        fs::write(root.join("app/api/ping/route.rs"), "pub async fn get() {} ").unwrap();
        fs::write(
            root.join("nextrs.toml"),
            "[app]\nname='t'\nurl='https://t.example'\n[vercel]\nregions=['pdx1']\n",
        )
        .unwrap();
        let plan_file = temp.path().join("plan.json");
        write_json(&plan_file, &plan(&root).unwrap()).unwrap();
        let fake = temp.path().join("fake-elf");
        fs::write(&fake, elf(62)).unwrap();
        let output = temp.path().join("out");
        // Stale content from a previous run must be cleared first.
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("stale"), "x").unwrap();

        let script = format!(
            "set -e; test \"$NEXTRS_SKIP_BUNDLE\" = 1; test \"$NEXTRS_BUILD_TARGET\" = vercel; \
             f=\"$NEXTRS_BUNDLE_OUTPUT/functions/__nextrs_functions/default.func\"; mkdir -p \"$f\"; \
             cp {} \"$f/executable\"; chmod +x \"$f/executable\"; \
             cp {} \"$NEXTRS_BUNDLE_OUTPUT/bundle-manifest.json\"",
            fake.display(),
            plan_file.display()
        );
        build_with_command(&root, &script, &output).unwrap();
        assert!(!output.join("stale").exists());
        let vc: Value = serde_json::from_slice(
            &fs::read(function_dir(&output, "default").join(".vc-config.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(vc["regions"], json!(["pdx1"]));
        assert!(output.join("config.json").is_file());

        let error = build_with_command(&root, "true", &output).unwrap_err();
        assert!(error.contains("broke a packaging rule"), "{error}");
        let error = build_with_command(&root, "exit 3", &output).unwrap_err();
        assert!(error.contains("failed with"), "{error}");
    }

    #[test]
    fn rejects_ambiguous_dynamic_routes_before_building() {
        let temp = tempfile::tempdir().unwrap();
        for name in ["[id]", "[slug]"] {
            let path = temp.path().join("app/items").join(name);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("route.rs"), "pub async fn get() {} ").unwrap();
        }
        assert!(
            plan(temp.path())
                .unwrap_err()
                .contains("ambiguous server routes")
        );
    }

    #[test]
    fn well_known_directories_use_the_same_discovery_as_the_router() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("app/.well-known");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("route.rs"), "pub async fn get() {} ").unwrap();
        fs::write(path.join("bundle.toml"), "bundle = 'metadata'").unwrap();
        assert_eq!(
            plan(temp.path()).unwrap().owners["/.well-known"],
            "metadata"
        );
    }
}
