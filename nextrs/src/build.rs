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
//! helpers. `middleware.rs` and `route.rs` are Rust-only conventions. `.rs`
//! wins when both are present.
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
        if let Some(p) = &route.middleware {
            emit_path_mod(&mut out, &mod_name(i, "middleware"), p);
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
        emit_middleware(&mut out, i, route);

        emit_methods(&mut out, i, route);
        out.push_str("    });\n");
    }

    out.push_str("    registry\n}\n");

    emit_openapi(&mut out, routes);

    out
}

/// Emit a `generated_openapi()` function returning a single
/// [`utoipa::openapi::OpenApi`] built from every `route.rs` method annotated
/// with `#[utoipa::path(...)]` or `#[nextrs::api(...)]`.
///
/// Annotation is opt-in: a handler without either attribute still routes
/// normally, it just doesn't appear in the spec (and therefore not in the
/// generated client). For an annotated handler, utoipa derives the request and
/// response schemas from the types named in the attribute and collects them
/// automatically — codegen only has to list the handler functions.
///
/// With raw `#[utoipa::path]`, the `path = "..."` must match the file-convention
/// URL (e.g. a handler in `app/api/ping/route.rs` uses `path = "/api/ping"`),
/// and codegen verifies that (see below). `#[nextrs::api]` derives the path from
/// the file, so there's nothing to write or to drift.
fn emit_openapi(out: &mut String, routes: &[DiscoveredRoute]) {
    let mut handler_paths: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for (i, route) in routes.iter().enumerate() {
        let Some(route_file) = &route.route else {
            continue;
        };
        let Ok(source) = std::fs::read_to_string(route_file) else {
            continue;
        };
        let module = mod_name(i, "route");
        for (fn_name, _) in ROUTE_METHODS {
            if has_public_async_method(&source, fn_name)
                && method_has_openapi_annotation(&source, fn_name)
            {
                handler_paths.push(format!("{}::{}", module, fn_name));

                // Guard against the one footgun of declaring the path by hand:
                // it drifting from the file-convention URL. If we can read the
                // `path = "..."` literal and it disagrees, fail the build with a
                // pointed message (same spirit as the page/route GET conflict).
                if let Some(declared) = extract_annotated_path(&source, fn_name) {
                    if declared != route.url_path {
                        errors.push(format!(
                            "nextrs: `{}()` in the route.rs for {url} declares \
                             #[utoipa::path(path = {declared:?})], but its file-convention \
                             path is {url:?}. Update the annotation to path = {url:?}.",
                            fn_name,
                            url = route.url_path,
                            declared = declared,
                        ));
                    }
                }
            }
        }
    }

    for msg in &errors {
        let _ = writeln!(out, "::core::compile_error!({:?});", msg);
    }

    out.push_str(
        "\n/// OpenAPI document built from every `#[utoipa::path]`-annotated route.rs handler.\n",
    );

    if handler_paths.is_empty() {
        // No annotated handlers — still expose the function so consumers can
        // unconditionally serve a (valid, empty) spec.
        out.push_str("pub fn generated_openapi() -> ::utoipa::openapi::OpenApi {\n");
        out.push_str("    let mut doc = ::utoipa::openapi::OpenApiBuilder::new().build();\n");
        out.push_str("    ::nextrs::openapi::normalize(&mut doc);\n");
        out.push_str("    doc\n");
        out.push_str("}\n");
        return;
    }

    out.push_str("#[derive(::utoipa::OpenApi)]\n");
    out.push_str("#[openapi(paths(\n");
    for p in &handler_paths {
        let _ = writeln!(out, "    {},", p);
    }
    out.push_str("))]\n");
    out.push_str("struct __NextrsOpenApi;\n\n");
    out.push_str("pub fn generated_openapi() -> ::utoipa::openapi::OpenApi {\n");
    out.push_str("    let mut doc = <__NextrsOpenApi as ::utoipa::OpenApi>::openapi();\n");
    out.push_str("    ::nextrs::openapi::normalize(&mut doc);\n");
    out.push_str("    doc\n");
    out.push_str("}\n");
}

/// Whether `fn_name` in `source` is immediately preceded by a
/// `#[utoipa::path(...)]` attribute. Intentionally lightweight (no Rust
/// parser), matching the rest of this module: it finds the function and walks
/// back to the nearest `#[utoipa::path`, rejecting the match if another `fn`
/// sits between them (i.e. the attribute belongs to an earlier function).
fn method_has_openapi_annotation(source: &str, fn_name: &str) -> bool {
    annotation_region(source, fn_name).is_some()
}

/// Byte index where the `pub async fn <fn_name>` declaration starts.
fn fn_decl_index(source: &str, fn_name: &str) -> Option<usize> {
    for prefix in [
        "pub async fn ",
        "pub(crate) async fn ",
        "pub(super) async fn ",
    ] {
        if let Some(idx) = source.find(&format!("{}{}", prefix, fn_name)) {
            return Some(idx);
        }
    }
    None
}

/// Attribute markers that put a `route.rs` method into the OpenAPI document:
/// `#[utoipa::path]` directly, or `#[nextrs::api]` (which expands to it with a
/// derived `path`).
const OPENAPI_ATTRS: &[&str] = &["#[utoipa::path", "#[nextrs::api"];

/// The slice of `source` spanning the OpenAPI attribute (`#[utoipa::path]` or
/// `#[nextrs::api]`) that immediately precedes `fn_name`, if there is one.
/// Returns `None` when the nearest such attribute is separated from the
/// function by another `fn` (i.e. it belongs to an earlier handler).
fn annotation_region<'a>(source: &'a str, fn_name: &str) -> Option<&'a str> {
    let idx = fn_decl_index(source, fn_name)?;
    let before = &source[..idx];
    let attr_pos = OPENAPI_ATTRS
        .iter()
        .filter_map(|marker| before.rfind(marker))
        .max()?;
    if before[attr_pos..].contains(" fn ") {
        return None;
    }
    Some(&source[attr_pos..idx])
}

/// Pull the `path = "..."` literal out of the `#[utoipa::path]` attribute for
/// `fn_name`. Deliberately lightweight (no Rust parser): it scans the attribute
/// for a `path` token — one bounded by `(`, `,`, or whitespace, so the
/// `utoipa::path` in the attribute name itself is skipped — followed by `=` and
/// a string literal. Returns `None` if no such literal is found (in which case
/// the caller skips validation rather than guessing).
fn extract_annotated_path(source: &str, fn_name: &str) -> Option<String> {
    let region = annotation_region(source, fn_name)?;
    let bytes = region.as_bytes();
    let is_ws = |b: u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r');

    let mut from = 0;
    while let Some(rel) = region[from..].find("path") {
        let p = from + rel;
        from = p + 4;

        // Must be a standalone `path` token, not the `::path` in the attribute
        // name (whose preceding byte is `:`).
        let boundary_before = p == 0 || matches!(bytes[p - 1], b'(' | b',') || is_ws(bytes[p - 1]);
        if !boundary_before {
            continue;
        }

        // Next non-whitespace byte must be `=`.
        let mut q = p + 4;
        while q < bytes.len() && is_ws(bytes[q]) {
            q += 1;
        }
        if q >= bytes.len() || bytes[q] != b'=' {
            continue;
        }

        // Then the opening quote of the string literal.
        let mut r = q + 1;
        while r < bytes.len() && is_ws(bytes[r]) {
            r += 1;
        }
        if r >= bytes.len() || bytes[r] != b'"' {
            continue;
        }
        let start = r + 1;
        let end_rel = region[start..].find('"')?;
        return Some(region[start..start + end_rel].to_string());
    }
    None
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

fn emit_middleware(out: &mut String, idx: usize, route: &DiscoveredRoute) {
    if route.middleware.is_some() {
        let _ = writeln!(
            out,
            "        middleware: Some(Box::new(|req| Box::pin({}::handle(req)))),",
            mod_name(idx, "middleware")
        );
    } else {
        out.push_str("        middleware: None,\n");
    }
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
                    } else if *file == "middleware.rs" {
                        "use axum::body::Body;\nuse http::Request;\npub async fn handle(req: Request<Body>) -> nextrs::conventions::MiddlewareResult { nextrs::conventions::MiddlewareResult::next(req) }\n"
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
            ("", &["layout.rs", "middleware.rs", "page.rs"]),
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
                && code.contains("mod __nextrs_route_0_middleware;")
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
        assert!(
            code.contains("middleware: Some(Box::new(|req| Box::pin(__nextrs_route_0_middleware::handle(req))))"),
            "expected middleware.rs handler wiring:\n{}",
            code
        );
        assert_eq!(
            code.matches("middleware: None,").count(),
            3,
            "expected middleware: None for routes without middleware:\n{}",
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
    fn unannotated_routes_emit_empty_openapi() {
        // A route.rs with no #[utoipa::path] still routes, but contributes
        // nothing to the spec — generated_openapi() falls back to the builder.
        let tmp = setup_app(&[("api/ping", &["route.rs"])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(code.contains("pub fn generated_openapi()"));
        assert!(
            code.contains("OpenApiBuilder::new().build()"),
            "expected empty-spec fallback:\n{}",
            code
        );
        assert!(
            !code.contains("__NextrsOpenApi"),
            "no derive struct when nothing is annotated:\n{}",
            code
        );
    }

    #[test]
    fn annotated_handlers_are_collected_into_openapi() {
        let route_body = r#"
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema)]
pub struct Pong { pub ok: bool }

#[utoipa::path(get, path = "/api/ping", responses((status = 200, body = Pong)))]
pub async fn get() -> Json<Pong> { Json(Pong { ok: true }) }

// Intentionally NOT annotated — should be excluded from the spec.
pub async fn post(_req: http::Request<axum::body::Body>) -> axum::http::StatusCode {
    axum::http::StatusCode::CREATED
}
"#;
        let tmp = setup_app_with_file_bodies(&[("api/ping", &[("route.rs", route_body)])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        // Both methods still route...
        assert!(code.contains("::nextrs::http::Method::GET"));
        assert!(code.contains("::nextrs::http::Method::POST"));

        // ...but only the annotated one is in the OpenAPI paths().
        assert!(code.contains("#[derive(::utoipa::OpenApi)]"));
        assert!(
            code.contains("__nextrs_route_0_route::get,"),
            "annotated get() should be in paths():\n{}",
            code
        );
        assert!(
            !code.contains("__nextrs_route_0_route::post,"),
            "unannotated post() must not be in paths():\n{}",
            code
        );
    }

    #[test]
    fn openapi_annotation_detection_is_per_function() {
        let src = r#"
#[utoipa::path(get, path = "/x")]
pub async fn get() -> &'static str { "x" }

pub async fn post() -> &'static str { "y" }
"#;
        assert!(method_has_openapi_annotation(src, "get"));
        assert!(!method_has_openapi_annotation(src, "post"));
    }

    #[test]
    fn extracts_annotated_path_ignoring_attribute_name() {
        // Multi-line attribute; the `path` token in `utoipa::path` must not be
        // mistaken for the `path = "..."` argument.
        let src = r#"
#[utoipa::path(
    post,
    path = "/api/ping",
    responses((status = 200, body = Pong)),
)]
pub async fn post() -> &'static str { "x" }
"#;
        assert_eq!(extract_annotated_path(src, "post").as_deref(), Some("/api/ping"));
    }

    #[test]
    fn no_path_literal_yields_none() {
        // request body inferred, path omitted entirely — nothing to validate.
        let src = "#[utoipa::path(post)]\npub async fn post() -> &'static str { \"x\" }\n";
        assert_eq!(extract_annotated_path(src, "post"), None);
    }

    #[test]
    fn mismatched_path_emits_compile_error() {
        let route_body = r#"
#[utoipa::path(get, path = "/wrong", responses((status = 200, body = Pong)))]
pub async fn get() -> axum::Json<Pong> { todo!() }
"#;
        let tmp = setup_app_with_file_bodies(&[("api/ping", &[("route.rs", route_body)])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(
            code.contains("compile_error!"),
            "expected a compile_error for the path mismatch:\n{}",
            code
        );
        assert!(
            code.contains("/api/ping") && code.contains("/wrong"),
            "compile_error should name both the declared and expected paths:\n{}",
            code
        );
    }

    #[test]
    fn nextrs_api_handlers_are_collected_without_path_check() {
        // `#[nextrs::api]` derives the path, so there's no literal to verify —
        // the handler is still collected into the spec, with no compile_error.
        let route_body = r#"
#[nextrs::api(get, responses((status = 200, body = Pong)))]
pub async fn get() -> axum::Json<Pong> { todo!() }
"#;
        let tmp = setup_app_with_file_bodies(&[("api/ping", &[("route.rs", route_body)])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(
            code.contains("__nextrs_route_0_route::get,"),
            "nextrs::api handler should be in the OpenAPI paths():\n{}",
            code
        );
        assert!(
            !code.contains("compile_error!"),
            "derived-path handler has nothing to validate:\n{}",
            code
        );
    }

    #[test]
    fn matching_path_emits_no_compile_error() {
        let route_body = r#"
#[utoipa::path(get, path = "/api/ping", responses((status = 200, body = Pong)))]
pub async fn get() -> axum::Json<Pong> { todo!() }
"#;
        let tmp = setup_app_with_file_bodies(&[("api/ping", &[("route.rs", route_body)])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(
            !code.contains("compile_error!"),
            "matching path should not error:\n{}",
            code
        );
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
