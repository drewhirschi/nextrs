use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One slot at a route segment — may have a `.rs` (logic) variant, an `.html`
/// (static) variant, a `.tsx` (React) variant, or none. Codegen preserves the
/// legacy `.rs`/`.html` path for existing apps, while `.tsx` is the active
/// frontend direction.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Slot {
    pub rs: Option<PathBuf>,
    pub html: Option<PathBuf>,
    pub tsx: Option<PathBuf>,
}

impl Slot {
    pub fn exists(&self) -> bool {
        self.rs.is_some() || self.html.is_some() || self.tsx.is_some()
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
    /// `not-found.{rs,html,tsx}` — the 404 surface for this segment's subtree.
    /// Rendered (wrapped in this segment's layouts) when no route under it
    /// matches. Like `page`, it may have a `.tsx` client-rendered variant;
    /// `.tsx` alongside `.rs`/`.html` is a codegen conflict.
    pub not_found: Slot,
    /// `middleware.rs` request guard — no `.html` variant.
    pub middleware: Option<PathBuf>,
    /// `route.rs` (API handler) — no `.html` variant.
    pub route: Option<PathBuf>,
    /// `props.rs` — server data for a `page.tsx` (Rust-only; requires the
    /// page slot to be `.tsx`, enforced by codegen).
    pub props: Option<PathBuf>,
}

/// Converts a directory name to a URL segment.
/// `[param]` becomes `{param}` (Axum dynamic segment syntax);
/// `[...param]` (Next.js catch-all) becomes `{*param}` (Axum wildcard —
/// matches one or more trailing segments, like Next's required catch-all).
fn dir_name_to_segment(name: &str) -> String {
    if name.starts_with('[') && name.ends_with(']') {
        let param = &name[1..name.len() - 1];
        if let Some(rest) = param.strip_prefix("...") {
            format!("{{*{}}}", rest)
        } else {
            format!("{{{}}}", param)
        }
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

/// Scan a directory tree for convention files. Returns discovered routes sorted
/// by URL path.
pub fn discover_routes(app_dir: &Path) -> Vec<DiscoveredRoute> {
    let mut routes = BTreeMap::new();
    scan_dir(app_dir, app_dir, &mut routes);

    routes.into_values().collect()
}

fn scan_dir(app_root: &Path, current: &Path, routes: &mut BTreeMap<String, DiscoveredRoute>) {
    let rel = current.strip_prefix(app_root).unwrap_or(Path::new(""));
    let url_path = rel_path_to_url(rel);

    let page = Slot {
        rs: optional_path(current, "page.rs"),
        html: optional_path(current, "page.html"),
        tsx: optional_path(current, "page.tsx"),
    };
    let layout = Slot {
        rs: optional_path(current, "layout.rs"),
        html: optional_path(current, "layout.html"),
        tsx: optional_path(current, "layout.tsx"),
    };
    let loading = Slot {
        rs: optional_path(current, "loading.rs"),
        html: optional_path(current, "loading.html"),
        tsx: optional_path(current, "loading.tsx"),
    };
    let not_found = Slot {
        rs: optional_path(current, "not-found.rs"),
        html: optional_path(current, "not-found.html"),
        tsx: optional_path(current, "not-found.tsx"),
    };
    let middleware = optional_path(current, "middleware.rs");
    let route = optional_path(current, "route.rs");
    let props = optional_path(current, "props.rs");

    if page.exists()
        || layout.exists()
        || loading.exists()
        || not_found.exists()
        || middleware.is_some()
        || route.is_some()
        || props.is_some()
    {
        routes.insert(
            url_path.clone(),
            DiscoveredRoute {
                url_path,
                dir: current.to_path_buf(),
                page,
                layout,
                loading,
                not_found,
                middleware,
                route,
                props,
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
    fn test_dynamic_and_catch_all_segments() {
        assert_eq!(dir_name_to_segment("[id]"), "{id}");
        assert_eq!(dir_name_to_segment("[...all]"), "{*all}");
        assert_eq!(dir_name_to_segment("plain"), "plain");

        let tmp = setup_app_dir(&[("api/auth/[...all]", &["route.rs"])]);
        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].url_path, "/api/auth/{*all}");
    }

    #[test]
    fn test_discover_nested_routes() {
        let tmp = setup_app_dir(&[
            ("", &["page.rs", "layout.rs"]),
            ("dashboard", &["page.rs", "layout.rs", "loading.rs"]),
            (
                "dashboard/settings",
                &["page.rs", "middleware.rs", "route.rs"],
            ),
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
        assert!(routes[2].middleware.is_some());
        assert!(routes[2].route.is_some());
    }

    #[test]
    fn test_discover_tsx_layout_and_loading() {
        let tmp = setup_app_dir(&[
            ("", &["layout.tsx", "loading.tsx"]),
            ("dashboard", &["page.tsx", "props.rs", "loading.tsx"]),
        ]);

        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].url_path, "/");
        assert!(routes[0].layout.tsx.is_some());
        assert!(routes[0].loading.tsx.is_some());
        assert_eq!(routes[1].url_path, "/dashboard");
        assert!(routes[1].page.tsx.is_some());
        assert!(routes[1].props.is_some());
        assert!(routes[1].loading.tsx.is_some());
    }

    #[test]
    fn test_discover_dynamic_segments() {
        let tmp = setup_app_dir(&[("users", &["page.rs"]), ("users/[id]", &["page.rs"])]);

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
    fn test_discover_middleware_only_segment() {
        let tmp = setup_app_dir(&[("", &["middleware.rs"]), ("reviews", &["middleware.rs"])]);

        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].url_path, "/");
        assert!(routes[0].middleware.is_some());
        assert_eq!(routes[1].url_path, "/reviews");
        assert!(routes[1].middleware.is_some());
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
        let tmp = setup_app_dir(&[("dashboard", &["page.html", "layout.html", "loading.html"])]);

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

    // -- React/TSX additions ---------------------------------------------------

    #[test]
    fn test_discover_tsx_page_and_props() {
        let tmp = setup_app_dir(&[("todos", &["page.tsx", "props.rs"])]);

        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 1);
        let r = &routes[0];
        assert_eq!(r.url_path, "/todos");
        assert!(r.page.exists());
        assert!(r.page.tsx.is_some());
        assert!(r.page.rs.is_none());
        assert!(r.props.is_some());
    }

    #[test]
    fn test_tsx_only_segment_registers_route() {
        let tmp = setup_app_dir(&[("dash", &["page.tsx"])]);

        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 1);
        assert!(routes[0].page.tsx.is_some());
        assert!(routes[0].props.is_none());
    }

    #[test]
    fn test_layout_and_loading_pick_up_tsx() {
        let tmp = setup_app_dir(&[("x", &["page.tsx", "layout.tsx", "loading.tsx"])]);

        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 1);
        assert!(routes[0].layout.tsx.is_some());
        assert!(routes[0].layout.exists());
        assert!(routes[0].loading.tsx.is_some());
        assert!(routes[0].loading.exists());
    }

    // -- not-found convention --------------------------------------------------

    #[test]
    fn test_discover_not_found_variants() {
        let tmp = setup_app_dir(&[
            ("", &["not-found.rs"]),
            ("admin", &["not-found.html"]),
            ("app", &["not-found.tsx"]),
        ]);

        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 3);

        // routes sort alphabetically: /, /admin, /app
        assert_eq!(routes[0].url_path, "/");
        assert!(routes[0].not_found.exists());
        assert!(routes[0].not_found.rs.is_some());

        assert_eq!(routes[1].url_path, "/admin");
        assert!(routes[1].not_found.html.is_some());

        assert_eq!(routes[2].url_path, "/app");
        assert!(routes[2].not_found.tsx.is_some());
    }

    #[test]
    fn test_not_found_only_segment_registers() {
        // A directory with ONLY a not-found file still registers a route.
        let tmp = setup_app_dir(&[("admin", &["not-found.rs"])]);

        let routes = discover_routes(tmp.path());
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].url_path, "/admin");
        assert!(!routes[0].page.exists());
        assert!(routes[0].not_found.rs.is_some());
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
