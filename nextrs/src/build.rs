//! Build-time codegen for nextrs apps. Feature-gated behind `build`.
//!
//! Call from a user crate's `build.rs`:
//!
//! ```ignore
//! fn main() {
//!     nextrs::build::emit_registry("app", "src/main.rs", "nextrs_routes.rs").unwrap();
//! }
//! ```
//!
//! Then in `main.rs`:
//!
//! ```ignore
//! include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));
//! ```
//!
//! `generated_registry()` is in scope after the include, returning a
//! `RouteRegistry` populated with every page/layout/loading slot the build.rs
//! discovered under `app/`. `.rs` slots are wired via `#[path]` mod
//! declarations; `.html` slots use `include_str!` + the framework's static
//! helpers. `.rs` wins when both are present.
//!
//! Lives in the framework crate (gated by the `build` feature) rather than a
//! separate `nextrs-build` workspace member because the codegen needs
//! `crate::discovery` — splitting them was just extra ceremony.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::discovery::{DiscoveredRoute, discover_routes};

const ROUTE_METHODS: &[(&str, &str)] = &[
    ("get", "GET"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
    ("head", "HEAD"),
    ("options", "OPTIONS"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct RouteMethod {
    fn_name: &'static str,
    method_const: &'static str,
}

/// Mirror a `public/` directory to a destination so a deploy target can serve
/// it. Call from a consumer crate's `build.rs`:
///
/// ```ignore
/// nextrs::build::sync_public_dir("site/public", "public")?;
/// ```
///
/// Both paths are interpreted relative to `CARGO_MANIFEST_DIR`. Files in
/// `src` are copied to `dst` only when missing or stale (mtime comparison);
/// files in `dst` that are not in `src` are removed so the mirror stays
/// authoritative. Hidden entries (names starting with `.`) are skipped.
///
/// The build.rs is also instructed to rerun whenever any file under `src`
/// changes. If `src` doesn't exist this is a no-op (returns `Ok(())`).
pub fn sync_public_dir(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set in build.rs"),
    );
    let abs_src = manifest_dir.join(src.as_ref());
    let abs_dst = manifest_dir.join(dst.as_ref());

    if !abs_src.is_dir() {
        return Ok(());
    }

    println!("cargo:rerun-if-changed={}", abs_src.display());

    std::fs::create_dir_all(&abs_dst)?;
    mirror_dir(&abs_src, &abs_dst)
}

fn mirror_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::collections::HashSet;

    let mut seen: HashSet<std::ffi::OsString> = HashSet::new();

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        seen.insert(name.clone());

        let from = entry.path();
        let to = dst.join(&name);
        let ft = entry.file_type()?;

        if ft.is_dir() {
            std::fs::create_dir_all(&to)?;
            mirror_dir(&from, &to)?;
        } else if ft.is_file() {
            let needs_copy = match (std::fs::metadata(&to), std::fs::metadata(&from)) {
                (Ok(dst_meta), Ok(src_meta)) => {
                    dst_meta.len() != src_meta.len()
                        || match (dst_meta.modified(), src_meta.modified()) {
                            (Ok(d), Ok(s)) => d < s,
                            _ => true,
                        }
                }
                _ => true,
            };
            if needs_copy {
                std::fs::copy(&from, &to)?;
            }
        }
    }

    // Prune entries in dst that no longer exist in src.
    for entry in std::fs::read_dir(dst)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') || seen.contains(&name) {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }

    Ok(())
}

/// Scan `app_dir` and emit a `generated_registry()` function into
/// `$OUT_DIR/<out_name>`.
///
/// `app_dir` is interpreted relative to `CARGO_MANIFEST_DIR`. The generated
/// `#[path = "..."]` attributes and `include_str!(...)` calls use absolute
/// paths to the convention files — necessary because `#[path]` inside an
/// `include!`'d file is resolved relative to the file's actual filesystem
/// location (in `OUT_DIR`), not the includer.
///
/// The build.rs is also instructed to rerun whenever any file under `app/`
/// changes.
pub fn emit_registry(
    app_dir: impl AsRef<Path>,
    _includer_path_unused: impl AsRef<Path>,
    out_name: &str,
) -> std::io::Result<()> {
    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set in build.rs"),
    );

    let abs_app = manifest_dir.join(app_dir.as_ref()).canonicalize()?;

    let routes = discover_routes(&abs_app);
    let code = generate_code(&routes);

    let out_dir = std::env::var_os("OUT_DIR").expect("OUT_DIR must be set in build.rs");
    let out_path = Path::new(&out_dir).join(out_name);
    std::fs::write(&out_path, &code)?;

    // Also dump a copy under target/nextrs/ for inspection — OUT_DIR is hashed
    // and hard to find by hand.
    if let Some(target_dir) = manifest_dir.ancestors().find_map(|p| {
        let candidate = p.join("target");
        if candidate.is_dir() {
            Some(candidate)
        } else {
            None
        }
    }) {
        let inspect_dir = target_dir.join("nextrs");
        if std::fs::create_dir_all(&inspect_dir).is_ok() {
            let _ = std::fs::write(inspect_dir.join(out_name), &code);
        }
    }

    println!("cargo:rerun-if-changed={}", abs_app.display());

    Ok(())
}

fn generate_code(routes: &[DiscoveredRoute]) -> String {
    let mut out = String::new();
    out.push_str("// @generated by nextrs::build. Do not edit by hand.\n\n");

    // ---- Module declarations for every .rs slot --------------------------
    for (i, route) in routes.iter().enumerate() {
        if let Some(p) = &route.page.rs {
            emit_path_mod(&mut out, &mod_name(i, "page"), p);
        }
        if let Some(p) = &route.layout.rs {
            emit_path_mod(&mut out, &mod_name(i, "layout"), p);
        }
        if let Some(p) = &route.loading.rs {
            emit_path_mod(&mut out, &mod_name(i, "loading"), p);
        }
        if let Some(p) = &route.route {
            emit_path_mod(&mut out, &mod_name(i, "route"), p);
        }
    }

    // ---- generated_registry() --------------------------------------------
    out.push_str("\npub fn generated_registry() -> ::nextrs::conventions::RouteRegistry {\n");
    out.push_str("    let mut registry = ::nextrs::conventions::RouteRegistry::new();\n");

    for (i, route) in routes.iter().enumerate() {
        let _ = writeln!(out, "    registry.add(::nextrs::conventions::RouteEntry {{");
        let _ = writeln!(out, "        path: {:?}.to_string(),", route.url_path);

        emit_slot(&mut out, "page", i, &route.page);
        emit_slot(&mut out, "layout", i, &route.layout);
        emit_slot(&mut out, "loading", i, &route.loading);

        emit_methods(&mut out, i, route);
        out.push_str("    });\n");
    }

    out.push_str("    registry\n}\n");
    out
}

fn discover_route_methods(path: &Path) -> Vec<RouteMethod> {
    let Ok(source) = std::fs::read_to_string(path) else {
        return Vec::new();
    };

    ROUTE_METHODS
        .iter()
        .filter_map(|(fn_name, method_const)| {
            if has_public_async_method(&source, fn_name) {
                Some(RouteMethod {
                    fn_name,
                    method_const,
                })
            } else {
                None
            }
        })
        .collect()
}

fn has_public_async_method(source: &str, fn_name: &str) -> bool {
    // This is intentionally lightweight. It recognizes the public async method
    // functions nextrs documents for route.rs without pulling a Rust parser
    // into the build feature.
    let needles = [
        format!("pub async fn {}", fn_name),
        format!("pub(crate) async fn {}", fn_name),
        format!("pub(super) async fn {}", fn_name),
    ];
    needles.iter().any(|needle| source.contains(needle))
}

fn emit_methods(out: &mut String, idx: usize, route: &DiscoveredRoute) {
    let Some(route_file) = &route.route else {
        out.push_str("        methods: vec![],\n");
        return;
    };

    let methods = discover_route_methods(route_file);
    if methods.is_empty() {
        out.push_str("        methods: vec![],\n");
        return;
    }

    if route.page.exists() && methods.iter().any(|m| m.method_const == "GET") {
        let _ = writeln!(
            out,
            "        methods: {{ compile_error!({:?}); vec![] }},",
            format!(
                "nextrs route conflict at {}: page.rs/page.html owns GET, so route.rs cannot export get()",
                route.url_path
            )
        );
        return;
    }

    let m = mod_name(idx, "route");
    out.push_str("        methods: vec![\n");
    for method in methods {
        let _ = writeln!(
            out,
            "            (::nextrs::http::Method::{}, ::nextrs::conventions::route_method({}::{})),",
            method.method_const, m, method.fn_name
        );
    }
    out.push_str("        ],\n");
}

fn mod_name(idx: usize, slot: &str) -> String {
    format!("__nextrs_route_{}_{}", idx, slot)
}

fn emit_path_mod(out: &mut String, name: &str, target: &Path) {
    // Absolute path. `#[path]` inside an `include!`'d file resolves relative
    // to the included file's location (OUT_DIR) — and since OUT_DIR is buried
    // many levels deep under `target/`, computing a stable relative path is
    // brittle. Absolute paths are unambiguous; rustc accepts them.
    let abs = target
        .canonicalize()
        .unwrap_or_else(|_| target.to_path_buf());
    let _ = writeln!(out, "#[path = {:?}]", abs.display().to_string());
    let _ = writeln!(out, "mod {};\n", name);
}

fn emit_slot(out: &mut String, slot: &str, idx: usize, s: &crate::discovery::Slot) {
    let m = mod_name(idx, slot);
    match (&s.rs, &s.html) {
        (Some(_), _) => {
            // .rs handler — call its render() via the path-included module.
            match slot {
                "page" => {
                    let _ = writeln!(
                        out,
                        "        page: Some(Box::new(|req| Box::pin({}::render(req)))),",
                        m
                    );
                }
                "layout" => {
                    let _ = writeln!(out, "        layout: Some(Box::new({}::render)),", m);
                }
                "loading" => {
                    let _ = writeln!(out, "        loading: Some(Box::new({}::render)),", m);
                }
                _ => unreachable!("unknown slot {}", slot),
            }
        }
        (None, Some(html)) => {
            // No .rs — bake the .html via include_str! + the static helper.
            // include_str! takes an absolute path; we pass it directly.
            let path_lit = format!("{:?}", html.display().to_string());
            match slot {
                "page" => {
                    let _ = writeln!(
                        out,
                        "        page: Some(::nextrs::conventions::static_page(include_str!({}))),",
                        path_lit
                    );
                }
                "layout" => {
                    let _ = writeln!(
                        out,
                        "        layout: Some(::nextrs::conventions::static_layout(include_str!({}))),",
                        path_lit
                    );
                }
                "loading" => {
                    let _ = writeln!(
                        out,
                        "        loading: Some(::nextrs::conventions::static_loading(include_str!({}))),",
                        path_lit
                    );
                }
                _ => unreachable!("unknown slot {}", slot),
            }
        }
        (None, None) => {
            let _ = writeln!(out, "        {}: None,", slot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_app(structure: &[(&str, &[&str])]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for (dir, files) in structure {
            let dir_path = tmp.path().join(dir);
            fs::create_dir_all(&dir_path).unwrap();
            for file in *files {
                let body = if file.ends_with(".rs") {
                    if *file == "route.rs" {
                        "use axum::body::Body;\nuse axum::response::IntoResponse;\nuse http::{Request, StatusCode};\npub async fn post(_req: Request<Body>) -> impl IntoResponse { StatusCode::OK }\n"
                    } else {
                        "pub fn render() -> String { String::new() }"
                    }
                } else {
                    "<html/>"
                };
                fs::write(dir_path.join(file), body).unwrap();
            }
        }
        tmp
    }

    fn setup_app_with_file_bodies(structure: &[(&str, &[(&str, &str)])]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for (dir, files) in structure {
            let dir_path = tmp.path().join(dir);
            fs::create_dir_all(&dir_path).unwrap();
            for (file, body) in *files {
                fs::write(dir_path.join(file), body).unwrap();
            }
        }
        tmp
    }

    /// Generated code mentions every route segment, attaches the right slot
    /// helpers based on .rs vs .html presence, and includes the marker
    /// comment so users can tell it was generated.
    #[test]
    fn generates_expected_skeleton() {
        let tmp = setup_app(&[
            ("", &["layout.rs", "page.rs"]),
            ("simple", &["page.rs"]),
            ("with-loading", &["page.rs", "loading.html"]),
            ("static-only", &["page.html", "layout.html"]),
        ]);

        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(code.starts_with("// @generated by nextrs::build."));

        // .rs slots produce #[path] mod declarations. Routes sort
        // alphabetically: /, /simple, /static-only, /with-loading — so the
        // .rs-bearing routes are at indices 0, 1, and 3 respectively.
        assert!(
            code.contains("#[path = ")
                && code.contains("mod __nextrs_route_0_page;")
                && code.contains("mod __nextrs_route_0_layout;")
                && code.contains("mod __nextrs_route_1_page;")
                && code.contains("mod __nextrs_route_3_page;"),
            "expected mod declarations missing:\n{}",
            code
        );

        // .html-only slots use static_* helpers via include_str!
        assert!(
            code.contains("static_page(include_str!"),
            "expected static_page macro for static-only page:\n{}",
            code
        );
        assert!(
            code.contains("static_layout(include_str!"),
            "expected static_layout macro for static-only layout:\n{}",
            code
        );
        assert!(
            code.contains("static_loading(include_str!"),
            "expected static_loading macro for with-loading.html:\n{}",
            code
        );

        // Each route appears as a registry.add(...)
        assert_eq!(code.matches("registry.add(").count(), 4);
        assert!(code.contains("\"/\".to_string()"));
        assert!(code.contains("\"/simple\".to_string()"));
        assert!(code.contains("\"/with-loading\".to_string()"));
        assert!(code.contains("\"/static-only\".to_string()"));
    }

    #[test]
    fn rs_takes_precedence_over_html_when_both_present() {
        let tmp = setup_app(&[("dual", &["page.rs", "page.html"])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        // Page slot uses the .rs module, NOT the .html helper.
        assert!(
            code.contains("Box::pin(__nextrs_route_0_page::render(req))"),
            "expected .rs to win:\n{}",
            code
        );
        assert!(
            !code.contains("static_page(include_str!"),
            "should not include the .html via static_page when .rs is present:\n{}",
            code
        );
    }

    #[test]
    fn emitted_paths_are_absolute() {
        // Both #[path] and include_str! must take absolute paths so that the
        // generated file (which lands in $OUT_DIR) can find them no matter
        // where in the target tree it ends up.
        let tmp = setup_app(&[("dyn", &["page.rs"]), ("static", &["page.html"])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        // Every #[path = "..."] must start with "/"
        for line in code.lines().filter(|l| l.starts_with("#[path =")) {
            assert!(
                line.contains("\"/"),
                "non-absolute path in generated code: {}",
                line
            );
        }

        // Every include_str!("...") must take an absolute path
        for line in code.lines().filter(|l| l.contains("include_str!")) {
            assert!(
                line.contains("include_str!(\"/"),
                "non-absolute include_str! in generated code: {}",
                line
            );
        }
    }

    #[test]
    fn route_rs_emits_module_and_methods() {
        let tmp = setup_app(&[("api/ping", &["route.rs"])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(
            code.contains("mod __nextrs_route_0_route;"),
            "expected route.rs module declaration:\n{}",
            code
        );
        assert!(
            code.contains("::nextrs::http::Method::POST"),
            "expected POST method entry:\n{}",
            code
        );
        assert!(
            code.contains("::nextrs::conventions::route_method(__nextrs_route_0_route::post)"),
            "expected generated handler to call post(req):\n{}",
            code
        );
        assert!(
            !code.contains("methods: vec![],"),
            "route.rs should not emit an empty methods vector:\n{}",
            code
        );
    }

    #[test]
    fn route_rs_supports_multiple_methods() {
        let route_body = r#"
use axum::body::Body;
use axum::response::IntoResponse;
use http::{Request, StatusCode};

pub async fn get(_req: Request<Body>) -> impl IntoResponse { StatusCode::OK }
pub async fn patch(_req: Request<Body>) -> impl IntoResponse { StatusCode::NO_CONTENT }
"#;
        let tmp = setup_app_with_file_bodies(&[("api/item/[id]", &[("route.rs", route_body)])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(code.contains("\"/api/item/{id}\".to_string()"));
        assert!(code.contains("::nextrs::http::Method::GET"));
        assert!(code.contains("::nextrs::http::Method::PATCH"));
        assert!(code.contains("route_method(__nextrs_route_0_route::get)"));
        assert!(code.contains("route_method(__nextrs_route_0_route::patch)"));
    }

    #[test]
    fn page_and_route_non_get_can_coexist() {
        let tmp = setup_app(&[("reviews", &["page.rs", "route.rs"])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(code.contains("\"/reviews\".to_string()"));
        assert!(code.contains("__nextrs_route_0_page::render(req)"));
        assert!(code.contains("::nextrs::http::Method::POST"));
        assert!(!code.contains("compile_error!"));
    }

    #[test]
    fn page_and_route_get_conflict_emits_compile_error() {
        let route_body = r#"
use axum::body::Body;
use axum::response::IntoResponse;
use http::{Request, StatusCode};

pub async fn get(_req: Request<Body>) -> impl IntoResponse { StatusCode::OK }
"#;
        let tmp = setup_app_with_file_bodies(&[(
            "reviews",
            &[
                ("page.rs", "pub fn render() -> String { String::new() }"),
                ("route.rs", route_body),
            ],
        )]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(code.contains("compile_error!"));
        assert!(code.contains("page.rs/page.html owns GET"));
    }
}
