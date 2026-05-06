use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One slot at a route segment — may have a `.rs` (logic) variant, an `.html`
/// (static) variant, both, or neither. Codegen prefers `.rs` when both exist.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Slot {
    pub rs: Option<PathBuf>,
    pub html: Option<PathBuf>,
}

impl Slot {
    pub fn exists(&self) -> bool {
        self.rs.is_some() || self.html.is_some()
    }
}

/// What convention files exist at a given route segment.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DiscoveredRoute {
    /// URL path (e.g. "/" or "/dashboard/settings")
    pub url_path: String,
    /// Filesystem directory containing these files
    pub dir: PathBuf,
    pub page: Slot,
    pub layout: Slot,
    pub loading: Slot,
    /// `route.rs` (API handler) — no `.html` variant.
    pub route: Option<PathBuf>,
}

/// Converts a directory name to a URL segment.
/// `[param]` becomes `{param}` (Axum dynamic segment syntax).
fn dir_name_to_segment(name: &str) -> String {
    if name.starts_with('[') && name.ends_with(']') {
        let param = &name[1..name.len() - 1];
        format!("{{{}}}", param)
    } else {
        name.to_string()
    }
}

/// Converts a filesystem path relative to the app root into a URL path.
/// e.g. "dashboard/settings" -> "/dashboard/settings"
/// e.g. "" (root) -> "/"
/// e.g. "users/[id]" -> "/users/{id}"
fn rel_path_to_url(rel: &Path) -> String {
    if rel.as_os_str().is_empty() {
        return "/".to_string();
    }

    let segments: Vec<String> = rel
        .components()
        .map(|c| dir_name_to_segment(c.as_os_str().to_str().unwrap_or("")))
        .collect();

    format!("/{}", segments.join("/"))
}

fn optional_path(dir: &Path, name: &str) -> Option<PathBuf> {
    let p = dir.join(name);
    if p.exists() { Some(p) } else { None }
}

/// Scan a directory tree for convention files (page.{rs,html}, layout.{rs,html},
/// loading.{rs,html}, route.rs). Returns discovered routes sorted by URL path.
pub fn discover_routes(app_dir: &Path) -> Vec<DiscoveredRoute> {
    let mut routes = BTreeMap::new();
    scan_dir(app_dir, app_dir, &mut routes);

    routes.into_values().collect()
}

fn scan_dir(
    app_root: &Path,
    current: &Path,
    routes: &mut BTreeMap<String, DiscoveredRoute>,
) {
    let rel = current.strip_prefix(app_root).unwrap_or(Path::new(""));
    let url_path = rel_path_to_url(rel);

    let page = Slot {
        rs: optional_path(current, "page.rs"),
        html: optional_path(current, "page.html"),
    };
    let layout = Slot {
        rs: optional_path(current, "layout.rs"),
        html: optional_path(current, "layout.html"),
    };
    let loading = Slot {
        rs: optional_path(current, "loading.rs"),
        html: optional_path(current, "loading.html"),
    };
    let route = optional_path(current, "route.rs");

    if page.exists() || layout.exists() || loading.exists() || route.is_some() {
        routes.insert(
            url_path.clone(),
            DiscoveredRoute {
                url_path,
                dir: current.to_path_buf(),
                page,
                layout,
                loading,
                route,
            },
        );
    }

    if let Ok(entries) = std::fs::read_dir(current) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                scan_dir(app_root, &entry.path(), routes);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_app_dir(structure: &[(&str, &[&str])]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for (dir, files) in structure {
            let dir_path = tmp.path().join(dir);
            fs::create_dir_all(&dir_path).unwrap();
            for file in *files {
                fs::write(dir_path.join(file), "// placeholder").unwrap();
            }
        }
        tmp
    }

    #[test]
    fn test_discover_root_page() {
        let tmp = setup_app_dir(&[("", &["page.rs"])]);

        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].url_path, "/");
        assert!(routes[0].page.exists());
        assert!(routes[0].page.rs.is_some());
        assert!(routes[0].page.html.is_none());
        assert!(!routes[0].layout.exists());
    }

    #[test]
    fn test_discover_nested_routes() {
        let tmp = setup_app_dir(&[
            ("", &["page.rs", "layout.rs"]),
            ("dashboard", &["page.rs", "layout.rs", "loading.rs"]),
            ("dashboard/settings", &["page.rs", "route.rs"]),
        ]);

        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 3);

        assert_eq!(routes[0].url_path, "/");
        assert!(routes[0].page.exists());
        assert!(routes[0].layout.exists());

        assert_eq!(routes[1].url_path, "/dashboard");
        assert!(routes[1].page.exists());
        assert!(routes[1].layout.exists());
        assert!(routes[1].loading.exists());

        assert_eq!(routes[2].url_path, "/dashboard/settings");
        assert!(routes[2].page.exists());
        assert!(routes[2].route.is_some());
    }

    #[test]
    fn test_discover_dynamic_segments() {
        let tmp = setup_app_dir(&[
            ("users", &["page.rs"]),
            ("users/[id]", &["page.rs"]),
        ]);

        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].url_path, "/users");
        assert_eq!(routes[1].url_path, "/users/{id}");
    }

    #[test]
    fn test_discover_api_routes() {
        let tmp = setup_app_dir(&[("api/users", &["route.rs"])]);

        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].url_path, "/api/users");
        assert!(!routes[0].page.exists());
        assert!(routes[0].route.is_some());
    }

    #[test]
    fn test_empty_dirs_ignored() {
        let tmp = setup_app_dir(&[
            ("", &["page.rs"]),
            ("empty", &[]),
            ("also-empty", &["README.md"]),
        ]);

        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].url_path, "/");
    }

    #[test]
    fn test_url_path_conversion() {
        assert_eq!(rel_path_to_url(Path::new("")), "/");
        assert_eq!(rel_path_to_url(Path::new("dashboard")), "/dashboard");
        assert_eq!(
            rel_path_to_url(Path::new("dashboard/settings")),
            "/dashboard/settings"
        );
        assert_eq!(rel_path_to_url(Path::new("users/[id]")), "/users/{id}");
        assert_eq!(
            rel_path_to_url(Path::new("users/[id]/posts/[postId]")),
            "/users/{id}/posts/{postId}"
        );
    }

    // -- Round 1 additions: .html files and .rs/.html pairing -----------------

    #[test]
    fn test_discover_html_only_segment() {
        let tmp = setup_app_dir(&[(
            "dashboard",
            &["page.html", "layout.html", "loading.html"],
        )]);

        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 1);
        let r = &routes[0];
        assert_eq!(r.url_path, "/dashboard");

        assert!(r.page.rs.is_none());
        assert!(r.page.html.is_some());
        assert!(r.layout.rs.is_none());
        assert!(r.layout.html.is_some());
        assert!(r.loading.rs.is_none());
        assert!(r.loading.html.is_some());
    }

    #[test]
    fn test_discover_rs_and_html_both_recorded() {
        let tmp = setup_app_dir(&[(
            "",
            &[
                "page.rs",
                "page.html",
                "layout.rs",
                "layout.html",
                "loading.rs",
                "loading.html",
            ],
        )]);

        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 1);
        let r = &routes[0];

        assert!(r.page.rs.is_some());
        assert!(r.page.html.is_some());
        assert!(r.layout.rs.is_some());
        assert!(r.layout.html.is_some());
        assert!(r.loading.rs.is_some());
        assert!(r.loading.html.is_some());
    }

    #[test]
    fn test_discover_mixed_nested() {
        let tmp = setup_app_dir(&[
            ("", &["layout.rs"]),
            ("dashboard", &["page.html", "loading.html"]),
            ("dashboard/settings", &["page.rs", "page.html"]),
        ]);

        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 3);

        assert_eq!(routes[0].url_path, "/");
        assert!(routes[0].layout.rs.is_some());
        assert!(routes[0].layout.html.is_none());
        assert!(!routes[0].page.exists());

        assert_eq!(routes[1].url_path, "/dashboard");
        assert!(routes[1].page.html.is_some());
        assert!(routes[1].page.rs.is_none());
        assert!(routes[1].loading.html.is_some());

        assert_eq!(routes[2].url_path, "/dashboard/settings");
        assert!(routes[2].page.rs.is_some());
        assert!(routes[2].page.html.is_some());
    }

    #[test]
    fn test_html_only_route_is_discovered() {
        // A segment with ONLY html files (no .rs at all) should still register.
        let tmp = setup_app_dir(&[("about", &["page.html"])]);

        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].url_path, "/about");
        assert!(routes[0].page.exists());
        assert!(routes[0].page.rs.is_none());
        assert!(routes[0].page.html.is_some());
    }
}
