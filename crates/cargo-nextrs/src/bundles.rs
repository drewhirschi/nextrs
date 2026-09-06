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
    fs::rename(&stage, &output).map_err(|e| e.to_string())?;
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
        let directory = stage.join(if options.vercel {
            format!("functions/__nextrs_functions/{}.func", bundle.name)
        } else {
            bundle.name.clone()
        });
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
        if options.vercel {
            // Same native executable runtime contract emitted by vercel-rust.
            let settings = crate::cron::load_config(root)?.vercel.unwrap_or_default();
            let mut config = json!({"handler":"executable","runtime":"executable","runtimeLanguage":"rust","architecture":"x86_64","environment":{},"supportsResponseStreaming":true});
            if !settings.regions.is_empty() {
                config["regions"] = json!(settings.regions);
            }
            write_json(&directory.join(".vc-config.json"), &config)?;
        }
    }
    write_json(&stage.join("bundle-manifest.json"), plan)?;
    write_json(&stage.join("routing.json"), &routing(plan))?;
    write_json(&stage.join("bundle-artifacts.json"), &artifacts)?;
    if options.vercel {
        if root.join("public").is_dir() {
            copy_tree(&root.join("public"), &stage.join("static"))?;
        }
        let crons: Vec<_> = crate::cron::discover_crons(root)?
            .into_iter()
            .filter(|c| c.provider() == crate::cron::Provider::Vercel)
            .map(|c| json!({"path":c.path,"schedule":c.schedule}))
            .collect();
        write_json(
            &stage.join("config.json"),
            &json!({"version":3,"routes":routing(plan),"crons":crons}),
        )?;
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
