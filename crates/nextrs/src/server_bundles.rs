//! Build-time server deployment boundaries, shared by codegen and the CLI.
//! A `bundle.toml` assigns a directory and its descendants to a named bundle.
//! `isolate = true` assigns only the colocated endpoint to a private bundle.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::discovery::{DiscoveredRoute, Slot};

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BundleSettings {
    #[serde(default)]
    pub features: Vec<String>,
    /// Files or directories, relative to the app root (no globs).
    #[serde(default)]
    pub assets: Vec<PathBuf>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentSettings {
    /// Cargo features enabled in every server bundle. Default features are off.
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Settings {
    #[serde(default)]
    bundles: BTreeMap<String, BundleSettings>,
    #[serde(default)]
    deployment: DeploymentSettings,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Declaration {
    bundle: Option<String>,
    isolate: Option<bool>,
    #[serde(default)]
    features: Vec<String>,
    #[serde(default)]
    assets: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    pub name: String,
    pub routes: Vec<String>,
    pub features: Vec<String>,
    pub assets: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundlePlan {
    pub version: u8,
    #[serde(default)]
    pub enabled: bool,
    pub bundles: BTreeMap<String, Bundle>,
    /// Includes convention-only segments to preserve codegen indices.
    pub owners: BTreeMap<String, String>,
}

fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.starts_with("route-")
}

pub fn relative_asset(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)))
}

fn declarations(dir: &Path, result: &mut BTreeMap<PathBuf, Declaration>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        if ty.is_dir() {
            declarations(&entry.path(), result)?;
        } else if entry.file_name() == "bundle.toml" {
            let declaration: Declaration = read_toml(&entry.path())?;
            match (&declaration.bundle, declaration.isolate) {
                (Some(name), None) if valid_name(name) => {}
                (None, Some(true)) => {}
                _ => {
                    return Err(format!(
                        "{}: use either bundle = \"name\" (lowercase letters, digits, hyphens; route- is reserved) or isolate = true",
                        entry.path().display()
                    ));
                }
            }
            result.insert(dir.to_path_buf(), declaration);
        }
    }
    Ok(())
}

impl BundlePlan {
    pub fn discover(root: &Path, app: &Path, routes: &[DiscoveredRoute]) -> Result<Self, String> {
        let settings: Settings = if root.join("nextrs.toml").is_file() {
            read_toml(&root.join("nextrs.toml"))?
        } else {
            Settings::default()
        };
        for (name, bundle) in &settings.bundles {
            if !valid_name(name) {
                return Err(format!("invalid bundle name {name:?}"));
            }
            for asset in &bundle.assets {
                if !relative_asset(asset) {
                    return Err(format!(
                        "bundle {name}: asset {} must be a relative file or directory without ..",
                        asset.display()
                    ));
                }
            }
        }
        let mut declarations_by_dir = BTreeMap::new();
        if app.is_dir() {
            declarations(app, &mut declarations_by_dir)?;
        }
        for (dir, declaration) in &declarations_by_dir {
            if declaration.bundle.is_some()
                && (!declaration.features.is_empty() || !declaration.assets.is_empty())
            {
                return Err(format!(
                    "{}: put named bundle features/assets in nextrs.toml",
                    dir.join("bundle.toml").display()
                ));
            }
            if declaration.assets.iter().any(|p| !relative_asset(p)) {
                return Err(format!(
                    "{}: assets must be relative files or directories without ..",
                    dir.join("bundle.toml").display()
                ));
            }
            let endpoints = routes
                .iter()
                .filter(|r| r.page.exists() || r.route.is_some());
            let exists = if declaration.isolate == Some(true) {
                endpoints.into_iter().any(|r| &r.dir == dir)
            } else {
                endpoints.into_iter().any(|r| r.dir.starts_with(dir))
            };
            if !exists {
                return Err(format!(
                    "{}: bundle declaration has no endpoint",
                    dir.join("bundle.toml").display()
                ));
            }
        }
        let mut isolated_settings = BTreeMap::new();
        let mut owners = BTreeMap::new();
        let mut groups: BTreeMap<String, Vec<String>> =
            BTreeMap::from([("default".into(), vec![])]);
        for route in routes {
            let mut owner = "default".to_string();
            for ancestor in route.dir.ancestors().take_while(|p| p.starts_with(app)) {
                if let Some(declaration) = declarations_by_dir.get(ancestor) {
                    if declaration.isolate == Some(true) {
                        if ancestor == route.dir {
                            // Stable, short identity; collisions are checked below.
                            let hash =
                                route.url_path.bytes().fold(0xcbf29ce484222325_u64, |h, b| {
                                    (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
                                });
                            owner = format!("route-{hash:016x}");
                            if isolated_settings
                                .insert(
                                    owner.clone(),
                                    BundleSettings {
                                        features: declaration.features.clone(),
                                        assets: declaration.assets.clone(),
                                    },
                                )
                                .is_some()
                            {
                                return Err(format!(
                                    "isolated bundle identity collision at {}",
                                    route.url_path
                                ));
                            }
                            break;
                        }
                    } else if let Some(name) = &declaration.bundle {
                        owner = name.clone();
                        break;
                    }
                }
            }
            if route.page.exists() || route.route.is_some() {
                groups
                    .entry(owner.clone())
                    .or_default()
                    .push(route.url_path.clone());
            }
            owners.insert(route.url_path.clone(), owner);
        }
        for name in settings.bundles.keys() {
            if !groups.contains_key(name) {
                return Err(format!(
                    "bundle {name:?} is configured but no endpoint uses it"
                ));
            }
        }
        let bundles = groups
            .into_iter()
            .map(|(name, routes)| {
                let options = settings
                    .bundles
                    .get(&name)
                    .or_else(|| isolated_settings.get(&name))
                    .cloned()
                    .unwrap_or_default();
                let features: BTreeSet<_> = settings
                    .deployment
                    .features
                    .iter()
                    .cloned()
                    .chain(options.features)
                    .collect();
                (
                    name.clone(),
                    Bundle {
                        name,
                        routes,
                        features: features.into_iter().collect(),
                        assets: options.assets,
                    },
                )
            })
            .collect();
        Ok(Self {
            version: 1,
            enabled: !declarations_by_dir.is_empty()
                || !settings.bundles.is_empty()
                || !settings.deployment.features.is_empty(),
            bundles,
            owners,
        })
    }

    pub fn is_split(&self) -> bool {
        self.bundles.len() > 1
    }

    /// Erase excluded modules before codegen, keeping stable module indices.
    /// Ancestor middleware/layout/loading/404 conventions travel with endpoints.
    pub fn select(
        &self,
        routes: &[DiscoveredRoute],
        name: &str,
    ) -> Result<Vec<DiscoveredRoute>, String> {
        let bundle = self
            .bundles
            .get(name)
            .ok_or_else(|| format!("unknown server bundle {name:?}"))?;
        let own_dirs: Vec<_> = routes
            .iter()
            .filter(|r| bundle.routes.contains(&r.url_path))
            .map(|r| &r.dir)
            .collect();
        let mut selected = routes.to_vec();
        for route in &mut selected {
            if self.owners.get(&route.url_path).map(String::as_str) != Some(name) {
                route.page = Slot::default();
                route.route = None;
                route.prefetch = None;
            }
            // The default function also owns unmatched-path rendering. Keep its
            // own fallback conventions even when it has no matching endpoint.
            let fallback = name == "default"
                && self.owners.get(&route.url_path).map(String::as_str) == Some(name);
            if !fallback && !own_dirs.iter().any(|dir| dir.starts_with(&route.dir)) {
                route.layout = Slot::default();
                route.loading = Slot::default();
                route.not_found = Slot::default();
                route.middleware = None;
            }
            if name != "default" && route.prefetch.is_some() {
                return Err(format!(
                    "{}: prefetch-backed pages must remain in the default bundle for now; cross-bundle prefetch is not supported",
                    route.url_path
                ));
            }
        }
        Ok(selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::discover_routes;
    fn put(root: &Path, path: &str, text: &str) {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    #[test]
    fn inherited_overridden_and_isolated_routes_keep_ancestors() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, "app/middleware.rs", "");
        put(root, "app/page.rs", "");
        put(root, "app/api/bundle.toml", "bundle = 'documents'");
        put(root, "app/api/middleware.rs", "");
        put(root, "app/api/pdf/route.rs", "");
        put(root, "app/api/ocr/bundle.toml", "isolate = true");
        put(root, "app/api/ocr/route.rs", "");
        put(root, "app/api/ocr/status/route.rs", "");
        put(root, "app/api/ping/bundle.toml", "bundle = 'default'");
        put(root, "app/api/ping/route.rs", "");
        let routes = discover_routes(&root.join("app"));
        let plan = BundlePlan::discover(root, &root.join("app"), &routes).unwrap();
        assert_eq!(plan.owners["/api/pdf"], "documents");
        assert_eq!(plan.owners["/api/ocr/status"], "documents");
        assert_eq!(plan.owners["/api/ping"], "default");
        assert!(plan.owners["/api/ocr"].starts_with("route-"));
        let selected = plan.select(&routes, "documents").unwrap();
        assert_eq!(selected.len(), routes.len());
        assert!(
            selected
                .iter()
                .find(|r| r.url_path == "/")
                .unwrap()
                .middleware
                .is_some()
        );
        assert!(
            selected
                .iter()
                .find(|r| r.url_path == "/")
                .unwrap()
                .page
                .rs
                .is_none()
        );
        assert!(
            selected
                .iter()
                .find(|r| r.url_path == "/api")
                .unwrap()
                .middleware
                .is_some()
        );
        assert!(
            selected
                .iter()
                .find(|r| r.url_path == "/api/ocr")
                .unwrap()
                .route
                .is_none()
        );
    }
    #[test]
    fn rejects_invalid_empty_and_unused_declarations() {
        for text in [
            "bundle = '../bad'",
            "bundle = 'route-abc'",
            "isolate = false",
            "bundle = 'pdf'\nisolate = true",
            "bundel = 'pdf'",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path();
            put(root, "app/route.rs", "");
            put(root, "app/bundle.toml", text);
            assert!(
                BundlePlan::discover(root, &root.join("app"), &discover_routes(&root.join("app")))
                    .is_err(),
                "{text}"
            );
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, "app/empty/bundle.toml", "bundle = 'pdf'");
        assert!(
            BundlePlan::discover(root, &root.join("app"), &[])
                .unwrap_err()
                .contains("no endpoint")
        );
        std::fs::remove_file(root.join("app/empty/bundle.toml")).unwrap();
        put(root, "nextrs.toml", "[bundles.typo]\nfeatures = []");
        assert!(
            BundlePlan::discover(root, &root.join("app"), &[])
                .unwrap_err()
                .contains("no endpoint uses")
        );
    }
    #[test]
    fn isolated_features_and_assets_do_not_leak_and_prefetch_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, "app/page.tsx", "");
        put(root, "app/video/route.rs", "");
        put(
            root,
            "app/video/bundle.toml",
            "isolate = true\nfeatures = ['video']\nassets = ['resources/video']",
        );
        let routes = discover_routes(&root.join("app"));
        let plan = BundlePlan::discover(root, &root.join("app"), &routes).unwrap();
        let isolated = &plan.bundles[&plan.owners["/video"]];
        assert_eq!(isolated.features, ["video"]);
        assert_eq!(isolated.assets, [PathBuf::from("resources/video")]);
        assert!(plan.bundles["default"].features.is_empty());
        assert!(plan.bundles["default"].assets.is_empty());
        put(root, "app/video/page.tsx", "");
        put(root, "app/video/prefetch.rs", "");
        let routes = discover_routes(&root.join("app"));
        let plan = BundlePlan::discover(root, &root.join("app"), &routes).unwrap();
        assert!(
            plan.select(&routes, &plan.owners["/video"])
                .unwrap_err()
                .contains("prefetch")
        );
    }

    #[test]
    fn default_bundle_settings_alone_enable_explicit_builds() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        put(root, "app/route.rs", "");
        put(root, "nextrs.toml", "[bundles.default]\nfeatures = ['api']");
        let routes = discover_routes(&root.join("app"));
        let plan = BundlePlan::discover(root, &root.join("app"), &routes).unwrap();
        assert!(plan.enabled);
        assert_eq!(plan.bundles["default"].features, ["api"]);
        assert!(plan.select(&routes, "missing").is_err());
    }
}
