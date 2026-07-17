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
//! `RouteRegistry` populated with every route slot the build.rs discovered
//! under `app/`. Legacy `.rs` slots are wired via `#[path]` mod declarations;
//! legacy `.html` slots use `include_str!` + the framework's static helpers.
//! React `.tsx` slots are bundled by `nextrs::bundle`.
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

    // Keep the normal typed-client route summary inspectable without making
    // healthy builds look warning-heavy. Set NEXTRS_VERBOSE=1 to echo it in
    // Cargo output while debugging codegen.
    println!("cargo:rerun-if-env-changed=NEXTRS_VERBOSE");
    print_client_summary_if_verbose(&routes);
    print_loading_warnings(&routes);

    // React convention files without the `tsx` feature: the shell handlers are
    // emitted either way, but nothing builds the bundles they point at.
    #[cfg(not(feature = "tsx"))]
    if routes.iter().any(|r| {
        r.page.tsx.is_some()
            || r.layout.tsx.is_some()
            || r.loading.tsx.is_some()
            || r.not_found.tsx.is_some()
    }) {
        println!(
            "cargo:warning=nextrs: .tsx convention files found but the `tsx` feature is not enabled — \
             enable it in [build-dependencies] and call nextrs::bundle::bundle_pages, or /dist/*.js will 404"
        );
    }

    let out_dir = std::env::var_os("OUT_DIR").expect("OUT_DIR must be set in build.rs");
    let out_path = Path::new(&out_dir).join(out_name);
    std::fs::write(&out_path, &code)?;

    // The registry refers to a generated asset table rather than hard-coding
    // stable /dist names. `bundle_pages` overwrites this fallback later in the
    // same build with Rolldown's actual content-hashed filenames. Keeping a
    // fallback makes the established emit_registry -> bundle_pages call order
    // work and leaves a useful diagnostic path when TSX bundling is omitted.
    let assets_path = Path::new(&out_dir).join("nextrs_assets.rs");
    if !assets_path.is_file() {
        let entries = fallback_asset_entries(&routes);
        let style = fallback_stylesheet_href();
        std::fs::write(
            &assets_path,
            asset_module_source(&entries, &routes, Some(&style))?,
        )?;
    }

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
            let _ = write_client_summary_file(&inspect_dir, out_name, &routes);
        }
    }

    println!("cargo:rerun-if-changed={}", abs_app.display());

    Ok(())
}

/// Emit `$OUT_DIR/<out_name>` re-exporting the typed seed companions that
/// `#[nextrs::api]` generates for eligible GET handlers, under names derived
/// from the route. A `prefetch.rs` includes this file and calls them:
///
/// ```ignore
/// include!(concat!(env!("OUT_DIR"), "/nextrs_seeds.rs"));
/// // → pub use crate::__nextrs_route_2_route as api_todos;
/// // → pub use crate::__nextrs_route_2_route::__nextrs_seed_get as get_api_todos;
/// ```
///
/// The module alias exists so param-struct *types* are reachable too
/// (`api_todos::TodosFilter`). Crate-root paths resolve because both the
/// registry and prefetch.rs land as crate-root modules in every consumer.
///
/// Eligibility mirrors the macro: an annotated `get` returning `Json<...>`
/// whose args are at most one `Path<...>` and one `Query<T>` extractor (in
/// any order, including none). Names come from the URL, not `operation_id`:
/// `/api/todos` → `get_api_todos` / `api_todos`, `/api/sources/{id}/pages` →
/// `get_api_sources_by_id_pages`.
pub fn emit_seeds(app_dir: impl AsRef<Path>, out_name: &str) -> std::io::Result<()> {
    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set in build.rs"),
    );
    let abs_app = manifest_dir.join(app_dir.as_ref()).canonicalize()?;
    let routes = discover_routes(&abs_app);

    let mut out = String::new();
    out.push_str("// @generated by nextrs::build::emit_seeds. Do not edit by hand.\n");
    for (i, route) in routes.iter().enumerate() {
        let Some(route_file) = &route.route else {
            continue;
        };
        let Ok(source) = std::fs::read_to_string(route_file) else {
            continue;
        };
        if has_public_async_method(&source, "get")
            && method_has_openapi_annotation(&source, "get")
            && get_is_seed_eligible(&source)
        {
            // pub(crate): the mangled route mods are crate-private; these
            // aliases are for sibling prefetch.rs modules in the same crate.
            let module = mod_name(i, "route");
            let alias = url_snake(&route.url_path);
            let _ = writeln!(
                out,
                "#[allow(unused_imports)]\npub(crate) use crate::{} as {};",
                module, alias
            );
            let _ = writeln!(
                out,
                "#[allow(unused_imports)]\npub(crate) use crate::{}::__nextrs_seed_get as get_{};",
                module, alias
            );
        }
    }

    let out_dir = std::env::var_os("OUT_DIR").expect("OUT_DIR must be set in build.rs");
    std::fs::write(Path::new(&out_dir).join(out_name), &out)?;
    if let Some(target_dir) = manifest_dir.ancestors().find_map(|p| {
        let candidate = p.join("target");
        candidate.is_dir().then_some(candidate)
    }) {
        let inspect_dir = target_dir.join("nextrs");
        if std::fs::create_dir_all(&inspect_dir).is_ok() {
            let _ = std::fs::write(inspect_dir.join(out_name), &out);
        }
    }
    Ok(())
}

/// `/api/todos` → `api_todos`, `/users/{id}` → `users_by_id`.
fn url_snake(url: &str) -> String {
    let parts: Vec<String> = url
        .split('/')
        .filter(|s| !s.is_empty())
        .map(
            |seg| match seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                Some(param) => format!("by_{}", param.to_lowercase()),
                None => seg.replace('-', "_").to_lowercase(),
            },
        )
        .collect();
    if parts.is_empty() {
        "root".to_string()
    } else {
        parts.join("_")
    }
}

/// Textual mirror of the macro's seed-companion eligibility: `get`'s args are
/// empty, or at most one `Path<...>` and one `Query<...>` extractor, and it
/// returns `Json<...>` or `Result<Json<...>, E>`. Kept deliberately simple —
/// a false positive means a clear "cannot find __nextrs_seed_get" error at
/// the `pub use`, not silent misrouting. The return check is a normalized
/// prefix match (NOT `contains("Json")`) so `Result<T, Json<E>>` or a type
/// named `JsonLines` can't sneak an alias past the macro.
fn get_is_seed_eligible(source: &str) -> bool {
    let Some(start) = source.find("pub async fn get") else {
        return false;
    };
    let sig_region = &source[start..];
    let Some(body_start) = sig_region.find('{') else {
        return false;
    };
    let sig = &sig_region[..body_start];

    let Some(open) = sig.find('(') else {
        return false;
    };
    let mut depth = 0usize;
    let mut close = None;
    for (i, c) in sig[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close) = close else { return false };
    // Multiline (rustfmt) signatures end the arg list with a trailing comma —
    // strip it so it isn't counted as an argument separator.
    let args = sig[open + 1..close].trim().trim_end_matches(',');
    let ret = &sig[close..];

    if !ret_is_seedable(ret) {
        return false;
    }
    let top_level_commas = {
        let mut depth = 0i32;
        let mut count = 0;
        for c in args.chars() {
            match c {
                '(' | '<' | '[' => depth += 1,
                ')' | '>' | ']' => depth -= 1,
                ',' if depth == 0 => count += 1,
                _ => {}
            }
        }
        count
    };
    if args.trim().is_empty() {
        return true;
    }
    // Every top-level arg must be a Path or Query extractor, at most one of
    // each — the shapes the macro's seed companion handles.
    let extractorish = args.contains("Query") || args.contains("Path");
    let arg_count = top_level_commas + 1;
    extractorish
        && arg_count <= 2
        && (arg_count == 1
            || (args.matches("Query").count() >= 1 && args.matches("Path").count() >= 1))
}

/// Whether the signature's return region (from the args' closing paren on)
/// names a companion-eligible type: `Json<...>` or `Result<Json<...>, _>`,
/// with optional module qualifiers (`axum::Json`). Mirrors the macro's
/// structural check so the two gatekeepers agree in both directions.
fn ret_is_seedable(ret: &str) -> bool {
    let normalized: String = ret.chars().filter(|c| !c.is_whitespace()).collect();
    let Some(ty) = normalized.strip_prefix(")->") else {
        return false; // no return type at all
    };
    fn strip_qualifiers(mut s: &str) -> &str {
        s = s.trim_start_matches("::");
        while let Some(pos) = s.find("::") {
            if s[..pos].chars().all(|c| c.is_alphanumeric() || c == '_') {
                s = &s[pos + 2..];
            } else {
                break;
            }
        }
        s
    }
    fn is_json_head(s: &str) -> bool {
        strip_qualifiers(s).starts_with("Json<")
    }
    let ty = strip_qualifiers(ty);
    is_json_head(ty)
        || ty
            .strip_prefix("Result<")
            .is_some_and(|ok_side| is_json_head(ok_side))
}

/// URL path → bundle entry name for `page.tsx` routes. `/` → `index`,
/// `/todos` → `todos`, `/users/{id}` → `users-_id_`. Shared between the shell
/// codegen here and `nextrs::bundle` so the script tag and the emitted bundle
/// can never disagree.
pub fn page_slug(url_path: &str) -> String {
    if url_path == "/" {
        return "index".to_string();
    }
    url_path
        .trim_start_matches('/')
        .replace('/', "-")
        .replace('{', "_")
        .replace('}', "_")
}

/// URL path → bundle entry name for a `not-found.tsx` route. Distinct from
/// [`page_slug`] so a segment can have both a `page.tsx` and a `not-found.tsx`
/// without their bundles colliding: `/` → `not-found`, `/admin` →
/// `admin-not-found`. Shared between the codegen here and `nextrs::bundle`.
pub fn not_found_slug(url_path: &str) -> String {
    if url_path == "/" {
        "not-found".to_string()
    } else {
        format!("{}-not-found", page_slug(url_path))
    }
}

fn generate_code(routes: &[DiscoveredRoute]) -> String {
    let mut out = String::new();
    out.push_str("// @generated by nextrs::build. Do not edit by hand.\n\n");
    out.push_str(
        "#[allow(dead_code)]\nmod __nextrs_assets {\n    include!(concat!(env!(\"OUT_DIR\"), \"/nextrs_assets.rs\"));\n}\n\n",
    );

    // ---- Convention conflicts ---------------------------------------------
    for route in routes {
        if route.page.tsx.is_some() && (route.page.rs.is_some() || route.page.html.is_some()) {
            let _ = writeln!(
                out,
                "::core::compile_error!({:?});",
                format!(
                    "nextrs page conflict at {}: page.tsx cannot coexist with page.rs/page.html — one rendering model per segment",
                    route.url_path
                )
            );
        }
        if route.layout.tsx.is_some() && (route.layout.rs.is_some() || route.layout.html.is_some())
        {
            let _ = writeln!(
                out,
                "::core::compile_error!({:?});",
                format!(
                    "nextrs layout conflict at {}: layout.tsx cannot coexist with layout.rs/layout.html — one layout model per segment",
                    route.url_path
                )
            );
        }
        if route.loading.tsx.is_some()
            && (route.loading.rs.is_some() || route.loading.html.is_some())
        {
            let _ = writeln!(
                out,
                "::core::compile_error!({:?});",
                format!(
                    "nextrs loading conflict at {}: loading.tsx cannot coexist with loading.rs/loading.html — one loading model per segment",
                    route.url_path
                )
            );
        }
        if route.not_found.tsx.is_some()
            && (route.not_found.rs.is_some() || route.not_found.html.is_some())
        {
            let _ = writeln!(
                out,
                "::core::compile_error!({:?});",
                format!(
                    "nextrs not-found conflict at {}: not-found.tsx cannot coexist with not-found.rs/not-found.html — one rendering model per segment",
                    route.url_path
                )
            );
        }
        if route.prefetch.is_some() && route.page.tsx.is_none() {
            let _ = writeln!(
                out,
                "::core::compile_error!({:?});",
                format!(
                    "nextrs: prefetch.rs/props.rs at {} requires a page.tsx sibling — prefetch data feeds a React page (Rust pages fetch their own data)",
                    route.url_path
                )
            );
        }
    }

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
        if let Some(p) = &route.not_found.rs {
            emit_path_mod(&mut out, &mod_name(i, "notfound"), p);
        }
        if let Some(p) = &route.middleware {
            emit_path_mod(&mut out, &mod_name(i, "middleware"), p);
        }
        if let Some(p) = &route.route {
            emit_path_mod(&mut out, &mod_name(i, "route"), p);
        }
        if let Some(p) = &route.prefetch {
            emit_path_mod(&mut out, &mod_name(i, "prefetch"), p);
        }
    }

    // ---- generated_registry() --------------------------------------------
    out.push_str("\npub fn generated_registry() -> ::nextrs::conventions::RouteRegistry {\n");
    out.push_str("    let mut registry = ::nextrs::conventions::RouteRegistry::new();\n");

    for (i, route) in routes.iter().enumerate() {
        let _ = writeln!(out, "    registry.add(::nextrs::conventions::RouteEntry {{");
        let _ = writeln!(out, "        path: {:?}.to_string(),", route.url_path);

        emit_page_slot(&mut out, i, route);
        emit_slot(&mut out, "layout", i, &route.layout);
        emit_loading_slot(&mut out, i, route, routes);
        emit_middleware(&mut out, i, route);

        emit_methods(&mut out, i, route);
        emit_prefetch_slot(&mut out, i, route);
        out.push_str("    });\n");
    }

    // React app-shell routes (page.tsx — same predicate as the shell's
    // NX_APP_ROUTES in bundle.rs): recorded so the router excludes them from
    // any injected Speculation Rules. The shell soft-navigates these URLs, so
    // a speculatively fetched document for them would never be used.
    for route in routes.iter() {
        if route.page.tsx.is_some() {
            let _ = writeln!(
                out,
                "    registry.mark_react_page({:?});",
                route.url_path
            );
        }
    }

    for (i, route) in routes.iter().enumerate() {
        emit_not_found(&mut out, i, route);
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

/// Per-method record of whether a `route.rs` handler made it into the OpenAPI
/// document (and therefore the generated client). Drives the build summary.
struct ClientStatus {
    /// HTTP method as it appears to the user, e.g. `GET`.
    method: &'static str,
    /// File-convention URL, e.g. `/api/ping`.
    url_path: String,
    /// `true` when the handler carries `#[nextrs::api]`/`#[utoipa::path]` and is
    /// thus part of the spec; `false` when it routes but isn't in the client.
    in_client: bool,
}

/// Walk every discovered `route.rs`, recording each public-async HTTP method
/// and whether it's annotated for the client. Reuses the same lightweight
/// detection as the codegen, so the summary can't disagree with what was
/// actually emitted into `paths(...)`.
fn client_status(routes: &[DiscoveredRoute]) -> Vec<ClientStatus> {
    let mut out = Vec::new();
    for route in routes {
        let Some(route_file) = &route.route else {
            continue;
        };
        let Ok(source) = std::fs::read_to_string(route_file) else {
            continue;
        };
        for (fn_name, method_const) in ROUTE_METHODS {
            if has_public_async_method(&source, fn_name) {
                out.push(ClientStatus {
                    method: method_const,
                    url_path: route.url_path.clone(),
                    in_client: method_has_openapi_annotation(&source, fn_name),
                });
            }
        }
    }
    out
}

fn has_prefetch_backed_tsx_page(route: &DiscoveredRoute) -> bool {
    route.page.tsx.is_some() && route.prefetch.is_some()
}

fn loading_tsx_applies_to_any_prefetch_route(
    routes: &[DiscoveredRoute],
    loading_route: &DiscoveredRoute,
) -> bool {
    routes.iter().any(|route| {
        has_prefetch_backed_tsx_page(route)
            && entry_applies_to_path(&loading_route.url_path, &route.url_path)
    })
}

/// Warn when a `loading.tsx` cannot affect any prefetch-backed React route. A
/// parent `app/loading.tsx` is valid when any descendant has `prefetch.rs`.
fn print_loading_warnings(routes: &[DiscoveredRoute]) {
    for route in routes.iter().filter(|route| route.loading.tsx.is_some()) {
        if !loading_tsx_applies_to_any_prefetch_route(routes, route) {
            println!(
                "cargo:warning=nextrs: {} has loading.tsx but no prefetch-backed page.tsx route uses it",
                route.url_path
            );
        }
    }
}

fn route_depth(path: &str) -> usize {
    if path == "/" {
        0
    } else {
        path.matches('/').count()
    }
}

fn entry_applies_to_path(entry_path: &str, target_path: &str) -> bool {
    entry_path == "/"
        || target_path == entry_path
        || target_path
            .strip_prefix(entry_path)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn nearest_tsx_loading<'a>(
    routes: &'a [DiscoveredRoute],
    target_path: &str,
) -> Option<&'a DiscoveredRoute> {
    routes
        .iter()
        .filter(|route| {
            route.loading.tsx.is_some() && entry_applies_to_path(&route.url_path, target_path)
        })
        .max_by_key(|route| route_depth(&route.url_path))
}

/// URL path → loading bundle entry name. `/` → `index.loading`.
pub fn loading_slug(url_path: &str) -> String {
    format!("{}.loading", page_slug(url_path))
}

fn tsx_loading_shell(loading_route: &DiscoveredRoute) -> String {
    let entry = loading_slug(&loading_route.url_path);
    format!("__nextrs_assets::loading({entry:?})")
}

pub(crate) fn tsx_loading_shell_with_src(src: &str, style_href: Option<&str>) -> String {
    format!(
        r#"{}<div id="__nx_loading_root__"></div><script>import({:?});</script>"#,
        tsx_document_head_with_href(style_href),
        src
    )
}

/// The stylesheet link every tsx shell carries. When the consuming app has a
/// `public/style.css`, its content hash becomes a cache-busting query so a
/// restyled deploy can't serve a stale sheet (browsers cache aggressively,
/// and dev serves without fingerprinting). No file → no query.
fn fallback_stylesheet_href() -> String {
    match std::env::var_os("CARGO_MANIFEST_DIR")
        .map(|dir| std::path::Path::new(&dir).join("public/style.css"))
        .filter(|p| p.is_file())
    {
        Some(css) => {
            println!("cargo:rerun-if-changed={}", css.display());
            let content = std::fs::read(&css).unwrap_or_default();
            format!("/style.css?v={}", content_hash(&content))
        }
        None => "/style.css".to_string(),
    }
}

fn tsx_document_head_with_href(style_href: Option<&str>) -> String {
    style_href
        .map(|href| format!(r#"<link rel="stylesheet" href="{href}" />"#))
        .unwrap_or_default()
}

/// A small stable content hash (FNV-1a, hex) — enough to change whenever the
/// file changes, with no new dependency.
pub(crate) fn content_hash(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn fallback_asset_entries(
    routes: &[DiscoveredRoute],
) -> std::collections::BTreeMap<String, String> {
    let mut entries = std::collections::BTreeMap::new();
    if routes.iter().any(|route| route.page.tsx.is_some()) {
        entries.insert(
            "__app_shell__".to_string(),
            "/dist/__app_shell__.js".to_string(),
        );
    }
    for route in routes {
        if route.loading.tsx.is_some() {
            let name = loading_slug(&route.url_path);
            entries.insert(name.clone(), format!("/dist/{name}.js"));
        }
        if route.not_found.tsx.is_some() {
            let name = not_found_slug(&route.url_path);
            entries.insert(name.clone(), format!("/dist/{name}.js"));
        }
    }
    entries
}

/// Rust source included by the generated registry. Keeping the complete shell
/// HTML here lets `bundle_pages` substitute Rolldown's actual hashed filenames
/// without requiring applications to reorder their build.rs calls.
pub(crate) fn asset_module_source(
    entries: &std::collections::BTreeMap<String, String>,
    routes: &[DiscoveredRoute],
    style_href: Option<&str>,
) -> std::io::Result<String> {
    use std::fmt::Write as _;

    let entry = |name: &str| {
        entries.get(name).map(String::as_str).ok_or_else(|| {
            std::io::Error::other(format!(
                "nextrs: bundled asset manifest is missing the {name:?} entry"
            ))
        })
    };

    let app_shell = if routes.iter().any(|route| route.page.tsx.is_some()) {
        tsx_shell_with_src(entry("__app_shell__")?, style_href)
    } else {
        String::new()
    };

    let mut out = String::from("// @generated by nextrs asset bundling. Do not edit.\n");
    let _ = writeln!(out, "pub const APP_SHELL: &str = {app_shell:?};");
    out.push_str("pub fn standalone(name: &str) -> &'static str {\n    match name {\n");
    for route in routes.iter().filter(|route| route.not_found.tsx.is_some()) {
        let name = not_found_slug(&route.url_path);
        let shell = tsx_shell_with_src(entry(&name)?, style_href);
        let _ = writeln!(out, "        {name:?} => {shell:?},");
    }
    out.push_str("        _ => panic!(\"nextrs: unknown standalone asset {name}\"),\n    }\n}\n");

    out.push_str("pub fn loading(name: &str) -> &'static str {\n    match name {\n");
    for route in routes.iter().filter(|route| route.loading.tsx.is_some()) {
        let name = loading_slug(&route.url_path);
        let shell = tsx_loading_shell_with_src(entry(&name)?, style_href);
        let _ = writeln!(out, "        {name:?} => {shell:?},");
    }
    out.push_str("        _ => panic!(\"nextrs: unknown loading asset {name}\"),\n    }\n}\n");
    Ok(out)
}

fn client_summary_text(routes: &[DiscoveredRoute]) -> Option<String> {
    let statuses = client_status(routes);
    if statuses.is_empty() {
        return None;
    }

    let in_client = statuses.iter().filter(|s| s.in_client).count();
    let mut out = format!(
        "nextrs: typed client generated for {in_client}/{} route.rs handler(s)\n",
        statuses.len()
    );

    for s in &statuses {
        let mark = if s.in_client {
            "client ✓"
        } else {
            "no client (add #[nextrs::api])"
        };
        let _ = writeln!(out, "  {:<7} {:<24} {}", s.method, s.url_path, mark);
    }

    Some(out)
}

fn client_summary_file_name(out_name: &str) -> String {
    let stem = Path::new(out_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("nextrs_routes");
    format!("{stem}.client-summary.txt")
}

fn write_client_summary_file(
    inspect_dir: &Path,
    out_name: &str,
    routes: &[DiscoveredRoute],
) -> std::io::Result<()> {
    let path = inspect_dir.join(client_summary_file_name(out_name));
    if let Some(summary) = client_summary_text(routes) {
        std::fs::write(path, summary)?;
    } else if let Err(err) = std::fs::remove_file(path) {
        if err.kind() != std::io::ErrorKind::NotFound {
            return Err(err);
        }
    }
    Ok(())
}

/// Print a per-handler client-codegen summary only when explicitly requested.
///
/// Build scripts have no stable informational output channel; `cargo:warning=`
/// is the only way to surface these lines in normal Cargo output. Keeping this
/// behind NEXTRS_VERBOSE prevents routine route summaries from being replayed
/// as warnings on healthy cached builds.
fn print_client_summary_if_verbose(routes: &[DiscoveredRoute]) {
    if !env_flag("NEXTRS_VERBOSE") {
        return;
    }

    let Some(summary) = client_summary_text(routes) else {
        return;
    };

    for line in summary.lines() {
        println!("cargo:warning={line}");
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.as_str(),
            "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
        )
    })
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
                "nextrs route conflict at {}: page owns GET, so route.rs cannot export get()",
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

/// Emit the `prefetch:` slot: the route's `prefetch.rs`/`props.rs` entry fn,
/// exposed for the soft-nav prefetch endpoint (`/__nx/prefetch?path=...`) so
/// client-side navigations can warm the query cache with the same entries a
/// hard load would stream. Dynamic routes extract params from the request the
/// same way the page handler does; both conventions' fn names are honored.
fn emit_prefetch_slot(out: &mut String, idx: usize, route: &DiscoveredRoute) {
    let Some(prefetch_path) = &route.prefetch else {
        out.push_str("        prefetch: None,\n");
        return;
    };
    let prefetch_mod = mod_name(idx, "prefetch");
    let entry_fn = if prefetch_path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "prefetch.rs")
    {
        "prefetch"
    } else {
        "props"
    };
    if route.url_path.contains('{') {
        let _ = writeln!(
            out,
            "        prefetch: Some(Box::new(|req| Box::pin(async move {{\n            \
             let (params, req) = ::nextrs::params::extract_params(req).await;\n            \
             {}::{}(req, params).await\n        \
             }}))),",
            prefetch_mod, entry_fn
        );
    } else {
        let _ = writeln!(
            out,
            "        prefetch: Some(Box::new(|req| Box::pin({}::{}(req)))),",
            prefetch_mod, entry_fn
        );
    }
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

/// Page slot, including the `.tsx` variant: a tsx page becomes a generated
/// shell handler — mount div + module script pointing at the bundle that
/// `nextrs::bundle::bundle_pages` produces for the same slug. The component
/// renders client-side; layouts/middleware/streaming behave as for any page.
///
/// With a `prefetch.rs` sibling, the handler awaits `prefetch(req)` and streams its
/// seeds as a JSON script tag ahead of the mount div — the await sits exactly
/// where a `page.rs` await would, so a loading slot still ships first.
fn emit_page_slot(out: &mut String, idx: usize, route: &DiscoveredRoute) {
    let is_tsx_only =
        route.page.tsx.is_some() && route.page.rs.is_none() && route.page.html.is_none();
    if !is_tsx_only {
        emit_slot(out, "page", idx, &route.page);
        return;
    }

    let shell = tsx_page_shell();
    // Dynamic segments (`[id]` → `{id}`) mean the request carries matched
    // params: stream them as a JSON tag ahead of the mount div, and hand them
    // to the prefetch/props fn (which takes `(req, params)` on dynamic routes).
    let is_dynamic = route.url_path.contains('{');
    if let Some(prefetch_path) = &route.prefetch {
        let prefetch_mod = mod_name(idx, "prefetch");
        // `prefetch.rs` exports `fn prefetch`; the legacy `props.rs` exports
        // `fn props`. Pick the entry fn by filename so both conventions work.
        let entry_fn = if prefetch_path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "prefetch.rs")
        {
            "prefetch"
        } else {
            "props"
        };
        if is_dynamic {
            let _ = writeln!(
                out,
                "        page: Some(Box::new(|req| Box::pin(async move {{\n            \
                 let (params, req) = ::nextrs::params::extract_params(req).await;\n            \
                 let seeds = {}::{}(req, params.clone()).await;\n            \
                 format!(\"{{}}{{}}{{}}\", params.to_script_tag(), seeds.to_script_tag(), {})\n        \
                 }}))),",
                prefetch_mod, entry_fn, shell
            );
        } else {
            let _ = writeln!(
                out,
                "        page: Some(Box::new(|req| Box::pin(async move {{\n            \
                 let seeds = {}::{}(req).await;\n            \
                 format!(\"{{}}{{}}\", seeds.to_script_tag(), {})\n        \
                 }}))),",
                prefetch_mod, entry_fn, shell
            );
        }
    } else if is_dynamic {
        let _ = writeln!(
            out,
            "        page: Some(Box::new(|req| Box::pin(async move {{\n            \
             let (params, _req) = ::nextrs::params::extract_params(req).await;\n            \
             format!(\"{{}}{{}}\", params.to_script_tag(), {})\n        \
             }}))),",
            shell
        );
    } else {
        let _ = writeln!(
            out,
            "        page: Some(::nextrs::conventions::static_page({})),",
            shell
        );
    }
}

// Every page.tsx document boots the SAME app-shell bundle (the single TanStack
// Router mount, generated by bundle::app_shell_entry) rather than a per-page
// bundle. So the documents for different routes are byte-identical except their
// `__nx_params__`/`__nx_seeds__` prefix; the shell's router reads the URL and
// renders the leaf, keeping shared layouts mounted across soft navigation.
// Keep this name in sync with the "__app_shell__" input name in bundle.rs.
fn tsx_page_shell() -> String {
    "__nextrs_assets::APP_SHELL".to_string()
}

/// Shell for standalone client mounts (`not-found.tsx`): boots the surface's
/// OWN bundle, not the app shell — a 404's URL is never in the shell's route
/// tree, and the shell's unknown-path fallback hard-reloads, which would loop.
fn tsx_standalone_shell(slug: &str) -> String {
    format!("__nextrs_assets::standalone({slug:?})")
}

pub(crate) fn tsx_shell_with_src(src: &str, style_href: Option<&str>) -> String {
    format!(
        r#"{}<div id="__nx_root__"></div><script>
// Define a global `process.env` BEFORE any module chunk runs. Zero-copy Next.js
// code (and TanStack Router) reads process.env.* at module-init; rolldown's
// `define` only rewrites NODE_ENV, and with code-splitting the env shim can land
// in a sibling chunk with no ordering guarantee — so set it here in an inline
// (non-module) script, which always runs before the deferred module script.
window.process || (window.process = {{ env: {{}} }});
(() => {{
  const renderError = (title, detail) => {{
    const root = document.getElementById("__nx_root__");
    if (!root || root.childElementCount > 0) return;
    const wrap = document.createElement("div");
    wrap.style.cssText = "box-sizing:border-box;margin:32px auto;max-width:860px;border:1px solid #f1a7a7;background:#fff7f7;color:#681414;border-radius:8px;padding:20px;font-family:ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;";
    const h = document.createElement("h1");
    h.textContent = title;
    h.style.cssText = "margin:0 0 10px;font-size:20px;line-height:1.2;";
    const pre = document.createElement("pre");
    pre.textContent = detail || "No error detail available.";
    pre.style.cssText = "margin:0;white-space:pre-wrap;overflow:auto;font:13px/1.5 ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,'Liberation Mono',monospace;";
    wrap.append(h, pre);
    root.append(wrap);
  }};
  window.addEventListener("error", (event) => {{
    renderError("Client-side page error", event.error?.stack || event.message || String(event));
  }});
  window.addEventListener("unhandledrejection", (event) => {{
    const reason = event.reason;
    renderError("Client-side async error", reason?.stack || reason?.message || String(reason));
  }});
}})();
</script><script type="module" src="{}"></script>"#,
        tsx_document_head_with_href(style_href),
        src
    )
}

/// Emit a `registry.add_not_found(path, render)` call for a segment's
/// `not-found.{rs,html,tsx}`, if it has one. The render is a
/// [`PageFn`](::nextrs::conventions::PageFn), exactly like a page:
///
/// - `.rs`   — calls the module's `pub async fn render(req) -> String`.
/// - `.html` — baked via `include_str!` + `static_page`.
/// - `.tsx`  — a client-rendered shell (the same one a `page.tsx` gets: mount
///   div, error boundary, stylesheet) pointing at the bundle `nextrs::bundle`
///   emits under [`not_found_slug`] — distinct from the page slug so a segment
///   can carry both a `page.tsx` and a `not-found.tsx`.
///
/// `.rs` wins over `.tsx` wins over `.html` when several are present; the
/// `.tsx`/`.rs`/`.html` combination is already rejected by the conflict check.
fn emit_not_found(out: &mut String, idx: usize, route: &DiscoveredRoute) {
    let s = &route.not_found;
    let path = &route.url_path;

    if s.rs.is_some() {
        let m = mod_name(idx, "notfound");
        let _ = writeln!(
            out,
            "    registry.add_not_found({:?}.to_string(), Box::new(|req| Box::pin({}::render(req))));",
            path, m
        );
    } else if s.tsx.is_some() {
        let shell = tsx_standalone_shell(&not_found_slug(path));
        let _ = writeln!(
            out,
            "    registry.add_not_found({:?}.to_string(), ::nextrs::conventions::static_page({}));",
            path, shell
        );
    } else if let Some(html) = &s.html {
        let path_lit = format!("{:?}", html.display().to_string());
        let _ = writeln!(
            out,
            "    registry.add_not_found({:?}.to_string(), ::nextrs::conventions::static_page(include_str!({})));",
            path, path_lit
        );
    }
}

fn emit_loading_slot(
    out: &mut String,
    idx: usize,
    route: &DiscoveredRoute,
    routes: &[DiscoveredRoute],
) {
    if route.loading.rs.is_some() || route.loading.html.is_some() {
        emit_slot(out, "loading", idx, &route.loading);
        return;
    }

    if has_prefetch_backed_tsx_page(route) {
        if let Some(loading_route) = nearest_tsx_loading(routes, &route.url_path) {
            let shell = tsx_loading_shell(loading_route);
            let _ = writeln!(
                out,
                "        loading: Some(::nextrs::conventions::static_loading({})),",
                shell
            );
            return;
        }
    }

    out.push_str("        loading: None,\n");
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
        assert_eq!(
            extract_annotated_path(src, "post").as_deref(),
            Some("/api/ping")
        );
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
    fn client_status_reports_per_method_inclusion() {
        let route_body = r#"
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct Pong { pub ok: bool }

#[nextrs::api(get, responses((status = 200, body = Pong)))]
pub async fn get() -> Json<Pong> { Json(Pong { ok: true }) }

// Unannotated — routes, but stays out of the client.
pub async fn post() -> axum::http::StatusCode { axum::http::StatusCode::CREATED }
"#;
        let tmp = setup_app_with_file_bodies(&[("api/ping", &[("route.rs", route_body)])]);
        let routes = discover_routes(tmp.path());
        let statuses = client_status(&routes);

        let get = statuses
            .iter()
            .find(|s| s.method == "GET")
            .expect("GET should be discovered");
        assert_eq!(get.url_path, "/api/ping");
        assert!(get.in_client, "annotated GET should be in the client");

        let post = statuses
            .iter()
            .find(|s| s.method == "POST")
            .expect("POST should be discovered");
        assert!(!post.in_client, "unannotated POST should be excluded");
    }

    #[test]
    fn client_summary_text_is_inspection_friendly() {
        let route_body = r#"
#[nextrs::api(get, responses((status = 200, body = Pong)))]
pub async fn get() -> axum::Json<Pong> { todo!() }

pub async fn post() -> axum::http::StatusCode { axum::http::StatusCode::CREATED }
"#;
        let tmp = setup_app_with_file_bodies(&[("api/ping", &[("route.rs", route_body)])]);
        let routes = discover_routes(tmp.path());
        let summary = client_summary_text(&routes).expect("route.rs handlers should summarize");

        assert!(summary.starts_with("nextrs: typed client generated for 1/2 route.rs handler(s)"));
        assert!(summary.contains("GET     /api/ping                client ✓"));
        assert!(
            summary.contains("POST    /api/ping                no client (add #[nextrs::api])")
        );
        assert_eq!(
            client_summary_file_name("nextrs_routes.rs"),
            "nextrs_routes.client-summary.txt"
        );
    }

    // -- React/TSX additions ---------------------------------------------------

    #[test]
    fn page_slug_shapes() {
        assert_eq!(page_slug("/"), "index");
        assert_eq!(page_slug("/todos"), "todos");
        assert_eq!(page_slug("/users/{id}"), "users-_id_");
        assert_eq!(page_slug("/a/b/c"), "a-b-c");
    }

    #[test]
    fn tsx_page_emits_shell_handler() {
        let tmp = setup_app(&[("todos", &["page.tsx"])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(
            code.contains("static_page(__nextrs_assets::APP_SHELL)"),
            "expected generated asset table reference:\n{}",
            code
        );
        assert!(!code.contains("compile_error!"), "{}", code);
    }

    #[test]
    fn tsx_pages_are_marked_react_for_speculation_exclusion() {
        // page.tsx routes are recorded on the registry so injected Speculation
        // Rules exclude them (the app shell soft-navigates those URLs).
        let tmp = setup_app(&[("", &["page.rs"]), ("todos", &["page.tsx"])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);
        assert!(
            code.contains(r#"registry.mark_react_page("/todos");"#),
            "expected react-page marker:\n{}",
            code
        );
        assert!(
            !code.contains(r#"registry.mark_react_page("/");"#),
            "page.rs route must not be marked react:\n{}",
            code
        );
    }

    #[test]
    fn tsx_loading_shell_includes_stylesheet_before_mount() {
        let tmp = setup_app(&[("", &["loading.tsx"]), ("todos", &["page.tsx", "props.rs"])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(
            code.contains("static_loading(__nextrs_assets::loading(\"index.loading\"))"),
            "expected generated loading asset reference:\n{}",
            code
        );
    }

    #[test]
    fn asset_module_bakes_hashed_script_and_stylesheet_urls_into_shells() {
        let tmp = setup_app(&[("", &["loading.tsx"]), ("todos", &["page.tsx"])]);
        let routes = discover_routes(tmp.path());
        let entries = std::collections::BTreeMap::from([
            (
                "__app_shell__".to_string(),
                "/dist/__app_shell__-abc123.js".to_string(),
            ),
            (
                "index.loading".to_string(),
                "/dist/index.loading-def456.js".to_string(),
            ),
        ]);

        let source =
            asset_module_source(&entries, &routes, Some("/dist/style-feedface.css")).unwrap();

        assert!(source.contains("/dist/__app_shell__-abc123.js"), "{source}");
        assert!(source.contains("/dist/index.loading-def456.js"), "{source}");
        assert!(source.contains("/dist/style-feedface.css"), "{source}");
        assert!(
            source.find("style-feedface.css").unwrap() < source.find("__nx_root__").unwrap(),
            "stylesheet must precede the React mount: {source}"
        );
        assert!(!source.contains("/style.css?v="), "{source}");
    }

    #[test]
    fn tsx_beside_rs_or_html_page_is_a_conflict() {
        for other in ["page.rs", "page.html"] {
            let tmp = setup_app(&[("todos", &["page.tsx", other])]);
            let routes = discover_routes(tmp.path());
            let code = generate_code(&routes);
            assert!(
                code.contains("compile_error!") && code.contains("one rendering model per segment"),
                "expected tsx/{} conflict:\n{}",
                other,
                code
            );
        }
    }

    #[test]
    fn tsx_page_with_props_awaits_props_and_streams_seeds() {
        let tmp = setup_app(&[("todos", &["page.tsx", "props.rs"])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(
            code.contains("mod __nextrs_route_0_prefetch;"),
            "expected props mod declaration:\n{}",
            code
        );
        assert!(
            code.contains("__nextrs_route_0_prefetch::props(req).await"),
            "expected props await in shell handler:\n{}",
            code
        );
        assert!(
            code.contains("seeds.to_script_tag()"),
            "expected seeds injection:\n{}",
            code
        );
        // Seeds JSON streams before the mount div.
        let handler = code
            .split("page: Some(")
            .nth(1)
            .expect("page handler emitted");
        assert!(
            handler.find("to_script_tag").unwrap()
                < handler.find("__nextrs_assets::APP_SHELL").unwrap(),
            "seeds must precede the mount div:\n{}",
            handler
        );
        assert!(!code.contains("compile_error!"), "{}", code);
    }

    #[test]
    fn prefetch_slot_emitted_for_prefetch_backed_routes() {
        // Static route with prefetch.rs → prefetch: Some(props-mod call).
        let tmp = setup_app(&[("todos", &["page.tsx", "prefetch.rs"])]);
        let code = generate_code(&discover_routes(tmp.path()));
        assert!(
            code.contains(
                "prefetch: Some(Box::new(|req| Box::pin(__nextrs_route_0_prefetch::prefetch(req))))"
            ),
            "{code}"
        );

        // Dynamic route → params extracted inside the closure.
        let tmp = setup_app(&[("todos/[id]", &["page.tsx", "prefetch.rs"])]);
        let code = generate_code(&discover_routes(tmp.path()));
        assert!(
            code.contains("prefetch: Some(Box::new(|req| Box::pin(async move {"),
            "{code}"
        );
        assert!(
            code.contains("__nextrs_route_0_prefetch::prefetch(req, params).await"),
            "{code}"
        );

        // Legacy props.rs keeps its fn name.
        let tmp = setup_app(&[("todos", &["page.tsx", "props.rs"])]);
        let code = generate_code(&discover_routes(tmp.path()));
        assert!(
            code.contains("Box::pin(__nextrs_route_0_prefetch::props(req))"),
            "{code}"
        );

        // No prefetch file → None.
        let tmp = setup_app(&[("about", &["page.tsx"])]);
        let code = generate_code(&discover_routes(tmp.path()));
        assert!(code.contains("prefetch: None,"), "{code}");
    }

    #[test]
    fn dynamic_tsx_page_with_props_gets_params() {
        let tmp = setup_app(&[("source/[id]", &["page.tsx", "props.rs"])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(
            code.contains("::nextrs::params::extract_params(req).await"),
            "expected params extraction:\n{}",
            code
        );
        assert!(
            code.contains("__nextrs_route_0_prefetch::props(req, params.clone()).await"),
            "expected props(req, params) call:\n{}",
            code
        );
        // Params tag streams before seeds, which stream before the mount div.
        let handler = code
            .split("page: Some(")
            .nth(1)
            .expect("page handler emitted");
        let params_at = handler.find("params.to_script_tag").unwrap();
        let seeds_at = handler.find("seeds.to_script_tag").unwrap();
        let root_at = handler.find("__nextrs_assets::APP_SHELL").unwrap();
        assert!(params_at < seeds_at && seeds_at < root_at, "{}", handler);
        assert!(!code.contains("compile_error!"), "{}", code);
    }

    #[test]
    fn dynamic_tsx_page_without_props_still_gets_params() {
        let tmp = setup_app(&[("source/[id]", &["page.tsx"])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(
            code.contains("::nextrs::params::extract_params(req).await"),
            "expected params extraction:\n{}",
            code
        );
        assert!(
            !code.contains("static_page"),
            "dynamic tsx page can't be a static shell:\n{}",
            code
        );
        assert!(!code.contains("compile_error!"), "{}", code);
    }

    #[test]
    fn static_tsx_page_without_props_stays_static() {
        let tmp = setup_app(&[("about", &["page.tsx"])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(code.contains("static_page"), "{}", code);
        assert!(!code.contains("extract_params"), "{}", code);
    }

    #[test]
    fn loading_tsx_is_inherited_by_prefetch_backed_tsx_pages() {
        let tmp = setup_app(&[
            ("", &["loading.tsx"]),
            ("dashboard", &["page.tsx", "props.rs"]),
            (
                "dashboard/reports",
                &["page.tsx", "props.rs", "loading.tsx"],
            ),
        ]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        let dashboard = code
            .split("\"/dashboard\".to_string()")
            .nth(1)
            .expect("dashboard route emitted");
        assert!(
            dashboard.contains(r#"__nextrs_assets::loading("index.loading")"#),
            "dashboard should inherit root loading.tsx:\n{}",
            dashboard
        );

        let reports = code
            .split("\"/dashboard/reports\".to_string()")
            .nth(1)
            .expect("reports route emitted");
        assert!(
            reports.contains(r#"__nextrs_assets::loading("dashboard-reports.loading")"#),
            "nearest loading.tsx should win:\n{}",
            reports
        );
    }

    #[test]
    fn loading_tsx_without_prefetch_backed_descendant_is_detected() {
        let tmp = setup_app(&[("", &["loading.tsx"]), ("about", &["page.tsx"])]);
        let routes = discover_routes(tmp.path());
        let root = routes.iter().find(|route| route.url_path == "/").unwrap();

        assert!(!loading_tsx_applies_to_any_prefetch_route(&routes, root));
    }

    #[test]
    fn tsx_layout_or_loading_beside_legacy_files_is_a_conflict() {
        for (slot, legacy) in [
            ("layout", "layout.rs"),
            ("layout", "layout.html"),
            ("loading", "loading.rs"),
            ("loading", "loading.html"),
        ] {
            let tsx = format!("{slot}.tsx");
            let tmp = setup_app(&[("dashboard", &[tsx.as_str(), legacy])]);
            let routes = discover_routes(tmp.path());
            let code = generate_code(&routes);
            assert!(
                code.contains("compile_error!") && code.contains("cannot coexist"),
                "expected {tsx}/{legacy} conflict:\n{}",
                code
            );
        }
    }

    #[test]
    fn props_without_tsx_page_is_a_conflict() {
        let tmp = setup_app(&[("todos", &["page.rs", "props.rs"])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);
        assert!(
            code.contains("compile_error!") && code.contains("requires a page.tsx sibling"),
            "{}",
            code
        );
    }

    // -- not-found convention --------------------------------------------------

    #[test]
    fn not_found_rs_emits_add_not_found_and_mod() {
        let tmp = setup_app_with_file_bodies(&[(
            "admin",
            &[(
                "not-found.rs",
                "use axum::body::Body;\nuse http::Request;\npub async fn render(_req: Request<Body>) -> String { \"missing\".into() }\n",
            )],
        )]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(
            code.contains("mod __nextrs_route_0_notfound;"),
            "expected not-found mod declaration:\n{}",
            code
        );
        assert!(
            code.contains(
                "registry.add_not_found(\"/admin\".to_string(), Box::new(|req| Box::pin(__nextrs_route_0_notfound::render(req))));"
            ),
            "expected add_not_found wiring:\n{}",
            code
        );
        assert!(!code.contains("compile_error!"), "{}", code);
    }

    #[test]
    fn not_found_html_emits_static_page_include() {
        let tmp = setup_app(&[("", &["not-found.html"])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(
            code.contains("registry.add_not_found(\"/\".to_string(), ::nextrs::conventions::static_page(include_str!("),
            "expected html not-found via static_page:\n{}",
            code
        );
    }

    #[test]
    fn not_found_tsx_emits_shell_with_not_found_slug() {
        let tmp = setup_app(&[("admin", &["not-found.tsx"])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);

        assert!(
            code.contains(r#"__nextrs_assets::standalone("admin-not-found")"#),
            "expected not-found bundle slug in shell:\n{}",
            code
        );
        assert!(
            code.contains(
                "registry.add_not_found(\"/admin\".to_string(), ::nextrs::conventions::static_page("
            ),
            "expected tsx shell wired via add_not_found:\n{}",
            code
        );
        assert!(!code.contains("compile_error!"), "{}", code);
    }

    #[test]
    fn not_found_tsx_beside_rs_is_a_conflict() {
        let tmp = setup_app(&[("admin", &["not-found.tsx", "not-found.rs"])]);
        let routes = discover_routes(tmp.path());
        let code = generate_code(&routes);
        assert!(
            code.contains("compile_error!") && code.contains("not-found conflict"),
            "expected not-found tsx/rs conflict:\n{}",
            code
        );
    }

    #[test]
    fn not_found_slug_shapes() {
        assert_eq!(not_found_slug("/"), "not-found");
        assert_eq!(not_found_slug("/admin"), "admin-not-found");
        assert_eq!(not_found_slug("/a/b"), "a-b-not-found");
    }

    #[test]
    fn url_snake_shapes() {
        assert_eq!(url_snake("/api/todos"), "api_todos");
        assert_eq!(url_snake("/users/{id}"), "users_by_id");
        assert_eq!(url_snake("/"), "root");
        assert_eq!(url_snake("/my-thing"), "my_thing");
    }

    #[test]
    fn seed_eligibility_mirrors_macro() {
        // Query extractor + Json return: eligible.
        assert!(get_is_seed_eligible(
            "pub async fn get(Query(f): Query<TodosFilter>) -> Json<Vec<Todo>> { todo!() }"
        ));
        // Zero args + Json return: eligible.
        assert!(get_is_seed_eligible(
            "pub async fn get() -> Json<PingResponse> { todo!() }"
        ));
        // Non-Json return: not eligible.
        assert!(!get_is_seed_eligible(
            "pub async fn get() -> impl IntoResponse { todo!() }"
        ));
        // Multiple extractors: not eligible.
        assert!(!get_is_seed_eligible(
            "pub async fn get(Query(f): Query<F>, headers: HeaderMap) -> Json<X> { todo!() }"
        ));
        // Raw request handler: not eligible.
        assert!(!get_is_seed_eligible(
            "pub async fn get(req: Request<Body>) -> Json<X> { todo!() }"
        ));
        // Path extractor: eligible.
        assert!(get_is_seed_eligible(
            "pub async fn get(Path(id): Path<i64>) -> Json<Vec<Page>> { todo!() }"
        ));
        // Path + Query, either order: eligible.
        assert!(get_is_seed_eligible(
            "pub async fn get(Path(id): Path<i64>, Query(f): Query<F>) -> Json<X> { todo!() }"
        ));
        assert!(get_is_seed_eligible(
            "pub async fn get(Query(f): Query<F>, Path(id): Path<i64>) -> Json<X> { todo!() }"
        ));
        // Path + non-extractor second arg: not eligible.
        assert!(!get_is_seed_eligible(
            "pub async fn get(Path(id): Path<i64>, headers: HeaderMap) -> Json<X> { todo!() }"
        ));
        // Fallible handlers: eligible — mirrors the macro's Result widening.
        assert!(get_is_seed_eligible(
            "pub async fn get() -> Result<Json<Vec<Todo>>, ApiError> { todo!() }"
        ));
        assert!(get_is_seed_eligible(
            "pub async fn get(Path(id): Path<u64>) -> Result<Json<TodoDetail>, StatusCode> { todo!() }"
        ));
        // Qualified paths still match.
        assert!(get_is_seed_eligible(
            "pub async fn get() -> axum::Json<X> { todo!() }"
        ));
        assert!(get_is_seed_eligible(
            "pub async fn get() -> Result<axum::Json<X>, E> { todo!() }"
        ));
        // Multiline rustfmt signatures carry a trailing comma — not an arg.
        assert!(get_is_seed_eligible(
            "pub async fn get(\n    Path(id): Path<u64>,\n    Query(q): Query<DetailQuery>,\n) -> Result<Json<TodoDetail>, StatusCode> { todo!() }"
        ));
        // Shapes that must NOT sneak past (the macro rejects them; an alias
        // here would be a "cannot find __nextrs_seed_get" build break).
        assert!(!get_is_seed_eligible(
            "pub async fn get() -> Result<StatusCode, Json<E>> { todo!() }"
        ));
        assert!(!get_is_seed_eligible(
            "pub async fn get() -> JsonLines<X> { todo!() }"
        ));
        assert!(!get_is_seed_eligible(
            "pub async fn get() -> impl IntoResponse { todo!() }"
        ));
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
        assert!(code.contains("page owns GET"));
    }
}
