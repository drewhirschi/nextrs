//! Build-time bundling of React convention files. Feature-gated behind `tsx`.
//!
//! For every `page.tsx` under the app dir, this emits a small entry wrapper
//! (layout composition + React Query provider + seed hydration + `createRoot`
//! mount) into `$OUT_DIR/nextrs_tsx/`. For every `loading.tsx`, it emits a
//! loading entry wrapper. Rolldown bundles all entries into a staging dir under
//! `$OUT_DIR`, then mirrors the result into `<public_dist>` with a byte-compare
//! so unchanged builds never touch the destination — `site/public` is watched
//! by both cargo (`rerun-if-changed` via `sync_public_dir`) and the dev watcher,
//! and unconditional writes would loop rebuilds/restarts.
//!
//! Call from a consumer crate's `build.rs`, after `emit_registry`:
//!
//! ```ignore
//! nextrs::bundle::bundle_pages(&nextrs::bundle::BundleConfig {
//!     app_dir: "app",
//!     client_dir: "client",
//!     client_alias: "@site/client",
//!     public_dist: "public/dist",
//! })?;
//! ```
//!
//! Production bundle names are content-addressed (`/dist/<slug>-<hash>.js`,
//! shared chunks under `/dist/chunks/`). The generated registry includes an
//! asset table written after Rolldown runs, so applications keep the familiar
//! `emit_registry()` then `bundle_pages()` build order.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub use crate::build::{loading_slug, not_found_slug, page_slug};
use crate::discovery::{DiscoveredRoute, discover_routes};

/// Configuration for [`bundle_pages`]. Directory paths are interpreted
/// relative to `CARGO_MANIFEST_DIR`.
///
/// Derives [`Default`] and asks callers to construct with `..Default::default()`
/// so new fields can be added without breaking them (a plain `#[non_exhaustive]`
/// would forbid the struct literal entirely, even with functional update):
///
/// ```ignore
/// nextrs::bundle::BundleConfig {
///     app_dir: "app",
///     client_dir: "client",
///     client_alias: "@site/client",
///     public_dist: "public/dist",
///     ..Default::default()
/// }
/// ```
#[derive(Default)]
pub struct BundleConfig<'a> {
    /// The app convention tree, e.g. `"app"`.
    pub app_dir: &'a str,
    /// The npm package holding node_modules and the generated client,
    /// e.g. `"client"`.
    pub client_dir: &'a str,
    /// Import specifier pages use for the generated client, aliased to
    /// `<client_dir>/src/index.ts`, e.g. `"@site/client"`.
    pub client_alias: &'a str,
    /// Where browser bundles land (served at `/dist/...`), e.g. `"public/dist"`.
    pub public_dist: &'a str,
    /// Extra rolldown resolve aliases as `(pattern, replacement)` pairs, on top
    /// of the built-in `@/*` → `<client_dir>/src/*` (which makes shadcn-style
    /// `@/lib/utils` / `@/components/ui/button` imports resolve). Replacements
    /// are relative to `client_dir`. Wildcard (`@/*` → `lib/*`) and prefix
    /// (`~` → `vendor`) forms both work; mirror them in `client/tsconfig.json`
    /// `paths` so `tsc` and the shadcn CLI agree with the bundler.
    pub aliases: &'a [(&'a str, &'a str)],
}

/// Content-addressed browser assets emitted by [`bundle_pages`]. URLs are
/// rooted at `/dist/`, matching `BundleConfig::public_dist`'s public mount.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BundleManifest {
    pub entries: BTreeMap<String, String>,
    pub stylesheet: Option<String>,
}

/// Discover `page.tsx` routes, bundle them, and mirror the output into
/// `<public_dist>`. No-op when the app has no `.tsx` pages.
pub fn bundle_pages(cfg: &BundleConfig) -> std::io::Result<BundleManifest> {
    // Escape hatch for the client-generation bootstrap: a brand-new page.tsx
    // may import hooks that `npm run gen` hasn't generated yet, while `npm run
    // gen` itself needs `cargo build` (for dump-openapi). The dump script sets
    // NEXTRS_SKIP_BUNDLE=1 to break the cycle.
    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set in build.rs"),
    );
    let abs_app = manifest_dir.join(cfg.app_dir).canonicalize()?;
    let routes = discover_routes(&abs_app);
    let dist = manifest_dir.join(cfg.public_dist);

    println!("cargo:rerun-if-env-changed=NEXTRS_SKIP_BUNDLE");
    if std::env::var_os("NEXTRS_SKIP_BUNDLE").is_some_and(|v| v == "1") {
        let manifest = manifest_from_existing_dist(&routes, &dist, &manifest_dir)?;
        write_asset_module(&routes, &manifest)?;
        return Ok(manifest);
    }

    let client_dir = manifest_dir.join(cfg.client_dir).canonicalize()?;

    // Rerun when client source or deps change. NOT node_modules (huge); a dep
    // bump edits package.json, which is enough.
    println!(
        "cargo:rerun-if-changed={}",
        client_dir.join("src").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        client_dir.join("package.json").display()
    );

    // URL-bound hook variants (useXFromUrl) for every GET with query params,
    // generated from the app's OpenAPI document — then the barrel, so both
    // exist before resolution needs them. Written by the same build that
    // bundles, so neither can go stale.
    emit_url_hooks(&client_dir)?;
    // Keep the generated-client barrel fresh: every module orval emitted gets
    // re-exported, so a hand-maintained list can never go stale.
    emit_client_barrel(&client_dir)?;

    let tsx_pages = page_bundles(&routes);
    let tsx_loadings: Vec<(String, PathBuf)> = routes
        .iter()
        .filter_map(|route| {
            route
                .loading
                .tsx
                .as_ref()
                .map(|path| (loading_slug(&route.url_path), path.clone()))
        })
        .collect();

    let tsx_not_founds = not_found_bundles(&routes);

    if tsx_pages.is_empty() && tsx_loadings.is_empty() && tsx_not_founds.is_empty() {
        // Prune a stale dist from a previous build that had tsx pages.
        if dist.is_dir() {
            std::fs::remove_dir_all(&dist)?;
        }
        let manifest = BundleManifest::default();
        write_asset_module(&routes, &manifest)?;
        return Ok(manifest);
    }

    let node_modules = client_dir.join("node_modules");
    if !node_modules.is_dir() {
        return Err(std::io::Error::other(format!(
            "nextrs: page.tsx pages found but {} is missing — run `npm install` in {}",
            node_modules.display(),
            client_dir.display()
        )));
    }

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR must be set"));
    let entries_dir = out_dir.join("nextrs_tsx");
    std::fs::create_dir_all(&entries_dir)?;

    let client_helper = client_dir.join("src/nextrs-client.ts");
    let mut inputs = Vec::with_capacity(tsx_pages.len() + tsx_loadings.len() + 1);

    // The ONE app-shell entry: a TanStack Router built from every discovered
    // route, mounted once. Every page.tsx document boots the manifest-resolved
    // __app_shell__ asset, so shared layout.tsx chrome stays mounted across
    // soft navigation and only the changed leaf swaps.
    let shell_path = entries_dir.join("__app_shell__.tsx");
    write_if_changed(
        &shell_path,
        app_shell_entry(&routes, &client_helper).as_bytes(),
    )?;
    inputs.push(rolldown::InputItem {
        name: Some("__app_shell__".to_string()),
        import: shell_path.display().to_string(),
    });
    // Each page.tsx is ALSO a named entry (the RAW page, no createRoot wrapper) so
    // it gets a named content-addressed chunk that the app-shell's lazy
    // `import("<abs page.tsx>")` dedups to (preserve_entry_signatures=False keeps
    // it from emitting an extra facade chunk).
    for page in &tsx_pages {
        inputs.push(rolldown::InputItem {
            name: Some(page.slug.clone()),
            import: page.page_path.display().to_string(),
        });
    }
    for (slug, loading_path) in &tsx_loadings {
        let entry_path = entries_dir.join(format!("{}.tsx", slug));
        let entry_src = loading_entry_wrapper(loading_path);
        write_if_changed(&entry_path, entry_src.as_bytes())?;
        inputs.push(rolldown::InputItem {
            name: Some(slug.clone()),
            import: entry_path.display().to_string(),
        });
    }
    // not-found.tsx mounts render outside the router (see NotFoundBundle), so
    // they keep the self-contained entry_wrapper boot.
    for nf in &tsx_not_founds {
        let entry_path = entries_dir.join(format!("{}.tsx", nf.slug));
        let entry_src = entry_wrapper(&nf.page_path, &nf.layout_paths, &client_helper);
        write_if_changed(&entry_path, entry_src.as_bytes())?;
        inputs.push(rolldown::InputItem {
            name: Some(nf.slug.clone()),
            import: entry_path.display().to_string(),
        });
    }

    let staging = out_dir.join("nextrs_dist");
    if staging.is_dir() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;

    let entries = run_bundler(inputs, &staging, &client_dir, cfg.client_alias, cfg.aliases)?;
    let stylesheet = fingerprint_stylesheet(&manifest_dir, &staging)?;
    let manifest = BundleManifest {
        entries,
        stylesheet,
    };
    write_if_changed(
        &staging.join("nextrs-assets.json"),
        serde_json::to_string_pretty(&manifest)
            .map_err(std::io::Error::other)?
            .as_bytes(),
    )?;
    write_asset_module(&routes, &manifest)?;

    std::fs::create_dir_all(&dist)?;
    mirror_by_content(&staging, &dist)?;
    Ok(manifest)
}

fn write_asset_module(
    routes: &[DiscoveredRoute],
    manifest: &BundleManifest,
) -> std::io::Result<()> {
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR must be set"));
    let source = crate::build::asset_module_source(
        &manifest.entries,
        routes,
        manifest.stylesheet.as_deref(),
    )?;
    write_if_changed(&out_dir.join("nextrs_assets.rs"), source.as_bytes())
}

fn manifest_from_existing_dist(
    routes: &[DiscoveredRoute],
    dist: &Path,
    manifest_dir: &Path,
) -> std::io::Result<BundleManifest> {
    let manifest_path = dist.join("nextrs-assets.json");
    if manifest_path.is_file() {
        let bytes = std::fs::read(&manifest_path)?;
        return serde_json::from_slice(&bytes).map_err(std::io::Error::other);
    }

    // The client-codegen bootstrap compiles dump-openapi with bundling skipped
    // before a brand-new app has any dist directory. Stable placeholders keep
    // the generated registry compilable; the subsequent normal build replaces
    // this asset table with the real Rolldown manifest before serving pages.
    if !dist.is_dir() {
        return Ok(BundleManifest {
            entries: expected_entry_names(routes)
                .into_iter()
                .map(|name| {
                    let href = format!("/dist/{name}.js");
                    (name, href)
                })
                .collect(),
            stylesheet: fallback_stylesheet(manifest_dir),
        });
    }

    // Backward-compatible bootstrap for an app's first upgrade: older
    // committed dist directories have stable entry names and no manifest.
    let mut entries = BTreeMap::new();
    for name in expected_entry_names(routes) {
        let stable = dist.join(format!("{name}.js"));
        if stable.is_file() {
            entries.insert(name.clone(), format!("/dist/{name}.js"));
            continue;
        }
        let prefix = format!("{name}-");
        let found = std::fs::read_dir(dist)?
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .find(|file| file.starts_with(&prefix) && file.ends_with(".js"));
        let Some(file) = found else {
            return Err(std::io::Error::other(format!(
                "nextrs: NEXTRS_SKIP_BUNDLE=1 but {dist:?} has no bundle for {name:?}; rebuild and commit public/dist"
            )));
        };
        entries.insert(name, format!("/dist/{file}"));
    }

    let stylesheet = std::fs::read_dir(dist)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .find(|file| file.starts_with("style-") && file.ends_with(".css"))
        .map(|file| format!("/dist/{file}"))
        .or_else(|| fallback_stylesheet(manifest_dir));

    Ok(BundleManifest {
        entries,
        stylesheet,
    })
}

fn fallback_stylesheet(manifest_dir: &Path) -> Option<String> {
    let css = manifest_dir.join("public/style.css");
    std::fs::read(css)
        .ok()
        .map(|bytes| format!("/style.css?v={}", crate::build::content_hash(&bytes)))
}

fn expected_entry_names(routes: &[DiscoveredRoute]) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    if routes.iter().any(|route| route.page.tsx.is_some()) {
        names.insert("__app_shell__".to_string());
    }
    for page in page_bundles(routes) {
        names.insert(page.slug);
    }
    for route in routes {
        if route.loading.tsx.is_some() {
            names.insert(loading_slug(&route.url_path));
        }
    }
    for not_found in not_found_bundles(routes) {
        names.insert(not_found.slug);
    }
    names
}

fn fingerprint_stylesheet(manifest_dir: &Path, staging: &Path) -> std::io::Result<Option<String>> {
    let source = manifest_dir.join("public/style.css");
    if !source.is_file() {
        return Ok(None);
    }
    println!("cargo:rerun-if-changed={}", source.display());
    let bytes = std::fs::read(source)?;
    let filename = format!("style-{}.css", crate::build::content_hash(&bytes));
    write_if_changed(&staging.join(&filename), &bytes)?;
    Ok(Some(format!("/dist/{filename}")))
}

#[derive(Debug, Clone)]
struct PageBundle {
    slug: String,
    page_path: PathBuf,
}

/// A standalone client-rendered mount: a `not-found.tsx`. These do NOT boot
/// the app shell — a 404's URL is never in the shell's route tree, and the
/// shell's unknown-path fallback is a hard reload, which would loop — so each
/// gets its own entry bundle composing the segment's layouts around it.
#[derive(Debug, Clone)]
struct NotFoundBundle {
    slug: String,
    page_path: PathBuf,
    layout_paths: Vec<PathBuf>,
}

fn not_found_bundles(routes: &[DiscoveredRoute]) -> Vec<NotFoundBundle> {
    routes
        .iter()
        .filter_map(|route| {
            Some(NotFoundBundle {
                slug: not_found_slug(&route.url_path),
                page_path: route.not_found.tsx.clone()?,
                layout_paths: collect_layouts_for_path(routes, &route.url_path),
            })
        })
        .collect()
}

/// Write `<client_dir>/src/generated/index.ts` re-exporting every generated
/// tag module plus `model` (and `url-hooks` when present), mirroring orval's
/// `tags-split` layout. No-op when the app has no generated client (dir
/// absent). Framework-owned so apps don't carry a barrel script — the codegen
/// output's surface is a framework concern, and this runs on every bundling
/// build so it can't go stale.
fn emit_client_barrel(client_dir: &Path) -> std::io::Result<()> {
    let generated = client_dir.join("src/generated");
    if !generated.is_dir() {
        return Ok(());
    }
    let mut tags: Vec<String> = std::fs::read_dir(&generated)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| name != "model")
        .collect();
    tags.sort();

    let mut out =
        String::from("// @generated by nextrs::bundle (every build). Do not edit by hand.\n");
    for tag in &tags {
        out.push_str(&format!("export * from \"./{tag}/{tag}\";\n"));
    }
    if generated.join("model").is_dir() {
        out.push_str("export * from \"./model\";\n");
    }
    if generated.join("url-hooks.ts").is_file() {
        out.push_str("export * from \"./url-hooks\";\n");
    }
    write_if_changed(&generated.join("index.ts"), out.as_bytes())
}

/// One GET operation that can carry a URL-bound hook variant: it has query
/// params, an operationId (orval derives `use<OperationId>` from it) and a
/// tag (orval's `tags-split` puts the hook in `./<tag>/<tag>`).
struct UrlHookOp {
    hook: String,
    tag: String,
    /// Path param names, in spec order. orval emits these as LEADING
    /// positional arguments on the hook (`useX(id, params?)`), so the wrapper
    /// takes them as explicit arguments — path params are identity (which
    /// thing; the page gets them from its route match), and only search
    /// params are URL-bound view state.
    path_keys: Vec<String>,
    query_keys: Vec<String>,
}

/// Generate `<client_dir>/src/generated/url-hooks.ts`: a `useXFromUrl()`
/// sibling for every GET with query params, implementing the "page search
/// params are the query params of its data" convention (see
/// docs/upstream-plans/url-bound-query-hooks.md and MANIFEST.md's data-flow
/// section):
///
/// - params come from the page URL, live via the app-shell router
///   (`useSearch`), so the React Query key derives from the URL and every
///   visited URL state is a warm cache entry;
/// - `setParams(patch)` is a soft navigation (`useNavigate`), so filters and
///   pagination are shareable/back-forwardable by construction;
/// - types derive from the orval hook itself (`Parameters<typeof useX>[0]`),
///   so nothing here guesses type names.
///
/// Reads `<client_dir>/openapi.json` (the artifact the app's `npm run dump`
/// writes). No-op when it's absent or contains no eligible operations —
/// a stale url-hooks.ts from a previous shape is removed.
fn emit_url_hooks(client_dir: &Path) -> std::io::Result<()> {
    let generated = client_dir.join("src/generated");
    let out_path = generated.join("url-hooks.ts");
    let spec_path = client_dir.join("openapi.json");

    let mut ops = if spec_path.is_file() {
        println!("cargo:rerun-if-changed={}", spec_path.display());
        url_hook_ops(&std::fs::read_to_string(&spec_path)?)
    } else {
        Vec::new()
    };
    // Only reference tag modules that actually exist — a torn generated dir
    // (interrupted `npm run gen`) should degrade to fewer wrappers, not a
    // confusing module-not-found at bundle time.
    ops.retain(|op| generated.join(&op.tag).is_dir());

    if ops.is_empty() || !generated.is_dir() {
        if out_path.is_file() {
            std::fs::remove_file(&out_path)?;
        }
        return Ok(());
    }

    write_if_changed(&out_path, url_hooks_source(&ops).as_bytes())
}

/// Extract the eligible operations from an OpenAPI document.
fn url_hook_ops(spec_json: &str) -> Vec<UrlHookOp> {
    let Ok(spec) = serde_json::from_str::<serde_json::Value>(spec_json) else {
        return Vec::new();
    };
    let Some(paths) = spec.get("paths").and_then(|p| p.as_object()) else {
        return Vec::new();
    };

    let mut ops = Vec::new();
    for item in paths.values() {
        let Some(get) = item.get("get") else { continue };
        let Some(op_id) = get.get("operationId").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(tag) = get
            .get("tags")
            .and_then(|t| t.as_array())
            .and_then(|t| t.first())
            .and_then(|t| t.as_str())
        else {
            continue;
        };
        let param_names = |kind: &str| -> Vec<String> {
            get.get("parameters")
                .and_then(|p| p.as_array())
                .map(|params| {
                    params
                        .iter()
                        .filter(|p| p.get("in").and_then(|i| i.as_str()) == Some(kind))
                        .filter_map(|p| p.get("name").and_then(|n| n.as_str()))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        };
        let path_keys = param_names("path");
        let query_keys = param_names("query");
        // No query params → nothing for the URL to bind; no wrapper.
        if query_keys.is_empty() {
            continue;
        }
        // orval: operationId `getTodos` → hook `useGetTodos`.
        let mut chars = op_id.chars();
        let hook = match chars.next() {
            Some(first) => format!("use{}{}", first.to_uppercase(), chars.as_str()),
            None => continue,
        };
        ops.push(UrlHookOp {
            hook,
            tag: tag.to_string(),
            path_keys,
            query_keys,
        });
    }
    ops.sort_by(|a, b| a.hook.cmp(&b.hook));
    ops
}

fn url_hooks_source(ops: &[UrlHookOp]) -> String {
    use std::fmt::Write as _;

    let mut out = String::from(
        r#"// @generated by nextrs::bundle (every build). Do not edit by hand.
//
// URL-bound hook variants: params come from the page URL (live via the
// app-shell's router), setParams(patch) is a soft navigation. State that
// defines the view — filters, sorts, pagination — belongs in the URL:
// shareable, refreshable, back/forward walks warm cache entries.
/* eslint-disable */
import { useNavigate, useSearch } from "@tanstack/react-router";
"#,
    );
    for op in ops {
        let _ = writeln!(
            out,
            "import {{ {hook} }} from \"./{tag}/{tag}\";",
            hook = op.hook,
            tag = op.tag
        );
    }

    out.push_str(
        r#"
type NxSearch = Record<string, unknown>;

function nxPick(search: NxSearch, keys: string[]): NxSearch {
  const out: NxSearch = {};
  for (const k of keys) if (search[k] !== undefined) out[k] = search[k];
  return out;
}

// undefined/null in a patch deletes the key from the URL.
function nxMerge(prev: NxSearch, patch: NxSearch): NxSearch {
  const next: NxSearch = { ...prev };
  for (const [k, v] of Object.entries(patch)) {
    if (v === undefined || v === null) delete next[k];
    else next[k] = v;
  }
  return next;
}
"#,
    );

    for op in ops {
        let hook = &op.hook;
        let keys = op
            .query_keys
            .iter()
            .map(|k| format!("{k:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        // orval puts path params first: useX(id, region, params?, ...). The
        // wrapper mirrors that — path values are explicit typed arguments
        // (positional types derived from the hook, no guessed names), and the
        // params object sits at index path_keys.len().
        let path_args = op
            .path_keys
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let ident = ts_ident(name);
                format!("{ident}: Parameters<typeof {hook}>[{i}]")
            })
            .collect::<Vec<_>>()
            .join(", ");
        let path_call = op
            .path_keys
            .iter()
            .map(|name| ts_ident(name))
            .collect::<Vec<_>>()
            .join(", ");
        let lead_args = if path_args.is_empty() {
            String::new()
        } else {
            format!("{path_args}, ")
        };
        let lead_call = if path_call.is_empty() {
            String::new()
        } else {
            format!("{path_call}, ")
        };
        let params_index = op.path_keys.len();
        let _ = write!(
            out,
            r#"
type {hook}Params = NonNullable<Parameters<typeof {hook}>[{params_index}]>;

export function {hook}FromUrl({lead_args}opts?: {{
  /** Params supplied by code, merged over the URL-derived ones. */
  fixed?: Partial<{hook}Params>;
  /** "replace" for typeahead-style updates; default pushes history. */
  history?: "push" | "replace";
}}) {{
  const search = useSearch({{ strict: false }}) as NxSearch;
  const params = {{
    ...nxPick(search, [{keys}]),
    ...opts?.fixed,
  }} as {hook}Params;
  const navigate = useNavigate();
  const setParams = (patch: Partial<{hook}Params>) =>
    navigate({{
      to: ".",
      search: (prev: NxSearch) => nxMerge(prev, patch as NxSearch),
      replace: opts?.history === "replace",
    }});
  const query = {hook}({lead_call}params);
  return {{ ...query, params, setParams }};
}}
"#
        );
    }
    out
}

/// A spec param name as a safe TS identifier for the wrapper's argument list.
/// Non-identifier characters become `_`; names colliding with the wrapper's
/// own locals get a trailing `_`.
fn ts_ident(name: &str) -> String {
    let mut ident: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if ident.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        ident.insert(0, '_');
    }
    if matches!(
        ident.as_str(),
        "opts" | "search" | "params" | "navigate" | "setParams" | "query" | "patch" | "prev"
    ) {
        ident.push('_');
    }
    ident
}

/// The `layout.tsx` chain that applies to `target_path`, root-first.
fn collect_layouts_for_path(routes: &[DiscoveredRoute], target_path: &str) -> Vec<PathBuf> {
    let mut layouts: Vec<&DiscoveredRoute> = routes
        .iter()
        .filter(|route| {
            route.layout.tsx.is_some() && entry_applies_to_path(&route.url_path, target_path)
        })
        .collect();
    layouts.sort_by_key(|route| route_depth(&route.url_path));
    layouts
        .into_iter()
        .filter_map(|route| route.layout.tsx.clone())
        .collect()
}

/// Raw browser entries for every `page.tsx`: the page module itself (no
/// wrapper — pages render inside the app shell, which lazy-imports the same
/// path and dedups to this content-addressed `/dist/<slug>-<hash>.js` chunk).
fn page_bundles(routes: &[DiscoveredRoute]) -> Vec<PageBundle> {
    routes
        .iter()
        .filter_map(|route| {
            Some(PageBundle {
                slug: page_slug(&route.url_path),
                page_path: route.page.tsx.clone()?,
            })
        })
        .collect()
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

fn layout_tree(layout_count: usize) -> String {
    let mut tree = String::new();
    for i in 0..layout_count {
        tree.push_str(&format!("<Layout{i}>"));
    }
    tree.push_str("<Page params={params} />");
    for i in (0..layout_count).rev() {
        tree.push_str(&format!("</Layout{i}>"));
    }
    tree
}

/// url_path -> `layout_<i>` ident for a route that owns a `layout.tsx`.
fn layout_ident_of(layouts: &[&DiscoveredRoute], url: &str) -> Option<String> {
    layouts
        .iter()
        .position(|l| l.url_path == url)
        .map(|i| format!("layout_{i}"))
}

/// Parent ident for `target`: the deepest layout route that applies to it. When
/// resolving a layout's OWN parent (`is_layout`), require a strictly shallower
/// layout so it can't parent itself. Defaults to `rootRoute`.
fn parent_ident_of(layouts: &[&DiscoveredRoute], target: &str, is_layout: bool) -> String {
    let mut best: Option<&DiscoveredRoute> = None;
    for &l in layouts {
        if is_layout && l.url_path == target {
            continue;
        }
        if !entry_applies_to_path(&l.url_path, target) {
            continue;
        }
        if is_layout && route_depth(&l.url_path) >= route_depth(target) {
            continue;
        }
        let better = match best {
            None => true,
            Some(b) => route_depth(&l.url_path) > route_depth(&b.url_path),
        };
        if better {
            best = Some(l);
        }
    }
    best.and_then(|l| layout_ident_of(layouts, &l.url_path))
        .unwrap_or_else(|| "rootRoute".to_string())
}

/// Map a discovery url_path to a JS regex literal matching concrete pathnames:
/// `{param}` → one segment, `{*rest}` → the remainder, literals escaped.
/// Drives the shell's click interceptor — only URLs the router can render are
/// soft-navigated; everything else stays a normal document load.
fn route_regex(url_path: &str) -> String {
    if url_path == "/" {
        return "/^\\/$/".to_string();
    }
    let mut re = String::from("/^");
    for seg in url_path.split('/').filter(|s| !s.is_empty()) {
        if seg.starts_with('(') && seg.ends_with(')') {
            continue;
        }
        re.push_str("\\/");
        if seg.starts_with("{*") && seg.ends_with('}') {
            re.push_str(".+");
        } else if seg.starts_with('{') && seg.ends_with('}') {
            re.push_str("[^\\/]+");
        } else {
            // Escape JS regex metacharacters in literal segments.
            for c in seg.chars() {
                if !c.is_ascii_alphanumeric() && c != '_' && c != '-' {
                    re.push('\\');
                }
                re.push(c);
            }
        }
    }
    re.push_str("$/");
    re
}

/// Map a discovery url_path (`{param}`, `{*param}`, `(group)`) to a TanStack
/// Router path: `{slug}`→`$slug`, `{*all}`→`$` (splat), `(group)` dropped.
fn tanstack_path(url_path: &str) -> String {
    if url_path == "/" {
        return "/".to_string();
    }
    let segs: Vec<String> = url_path
        .split('/')
        .filter(|s| !s.is_empty())
        .filter_map(|seg| {
            if seg.starts_with('(') && seg.ends_with(')') {
                None
            } else if seg.starts_with("{*") && seg.ends_with('}') {
                Some("$".to_string())
            } else if let Some(p) = seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                Some(format!("${p}"))
            } else {
                Some(seg.to_string())
            }
        })
        .collect();
    format!("/{}", segs.join("/"))
}

/// Deepest `loading.tsx` that applies to `target` (its route pendingComponent).
fn nearest_tsx_loading_route<'a>(
    routes: &'a [DiscoveredRoute],
    target: &str,
) -> Option<&'a DiscoveredRoute> {
    routes
        .iter()
        .filter(|r| r.loading.tsx.is_some() && entry_applies_to_path(&r.url_path, target))
        .max_by_key(|r| route_depth(&r.url_path))
}

/// Recursively emit `name.addChildren([..])` for nodes with children, else just
/// `name`. Deterministic — children are insertion-ordered Vecs.
fn emit_route_node(
    name: &str,
    children: &std::collections::HashMap<String, Vec<String>>,
) -> String {
    match children.get(name) {
        Some(kids) if !kids.is_empty() => {
            let inner = kids
                .iter()
                .map(|k| emit_route_node(k, children))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}.addChildren([{inner}])")
        }
        _ => name.to_string(),
    }
}

/// Entry wrapper for standalone client mounts (`not-found.tsx`): layout
/// composition + React Query provider + seed hydration + `createRoot` mount.
/// Pages no longer use this — they render inside the app shell — but a
/// not-found surface renders OUTSIDE the router (its URL is never in the
/// shell's route tree, whose unknown-path fallback hard-reloads), so it keeps
/// the self-contained boot. Params come from the server's `__nx_params__`
/// tag, which is always fresh here: a not-found document is only ever a hard
/// load.
fn entry_wrapper(page_path: &Path, layout_paths: &[PathBuf], client_helper: &Path) -> String {
    let layout_imports = layout_paths
        .iter()
        .enumerate()
        .map(|(i, path)| format!("import Layout{i} from \"{}\";\n", path.display()))
        .collect::<String>();
    let tree = layout_tree(layout_paths.len());
    format!(
        r#"// @generated by nextrs::bundle. Do not edit by hand.
import {{ createRoot }} from "react-dom/client";
import {{ QueryClient, QueryClientProvider }} from "@tanstack/react-query";
import {{ seedQueryClient }} from "{helper}";
import Page from "{page}";
{layout_imports}

// staleTime > 0 so server-seeded entries (see prefetch.rs) render without an
// immediate background refetch; with no seeds this is just a sane default.
const qc = new QueryClient({{
  defaultOptions: {{ queries: {{ staleTime: 30_000 }} }},
}});
seedQueryClient(qc);

// Matched route params ([seg] segments), streamed by the server as a JSON
// tag. Empty object on static routes.
const paramsEl = document.getElementById("__nx_params__");
const params = paramsEl?.textContent ? JSON.parse(paramsEl.textContent) : {{}};

createRoot(document.getElementById("__nx_root__")!).render(
  <QueryClientProvider client={{qc}}>
    {tree}
  </QueryClientProvider>,
);
"#,
        helper = client_helper.display(),
        page = page_path.display(),
        layout_imports = layout_imports,
        tree = tree,
    )
}

/// The single app-shell entry: a TanStack Router built from every discovered
/// route, mounted ONCE into `#__nx_root__`. Each `layout.tsx` becomes a pathless
/// layout route rendering the layout around an `<Outlet/>` (so it stays mounted
/// across soft navigation); each `page.tsx` becomes a lazily-loaded leaf at its
/// full path, receiving the router's LIVE matched params as its `params` prop —
/// the server's `__nx_params__` tag is only the boot-time hand-off and goes
/// stale across soft navigation. Replaces the per-page `entry_wrapper` for
/// pages (not-found keeps it).
fn app_shell_entry(routes: &[DiscoveredRoute], client_helper: &Path) -> String {
    use std::collections::HashMap;
    use std::fmt::Write as _;

    let mut layouts: Vec<&DiscoveredRoute> =
        routes.iter().filter(|r| r.layout.tsx.is_some()).collect();
    layouts.sort_by(|a, b| {
        route_depth(&a.url_path)
            .cmp(&route_depth(&b.url_path))
            .then_with(|| a.url_path.cmp(&b.url_path))
    });
    let mut pages: Vec<&DiscoveredRoute> = routes.iter().filter(|r| r.page.tsx.is_some()).collect();
    pages.sort_by(|a, b| a.url_path.cmp(&b.url_path));

    let mut out = String::new();
    out.push_str(
        "// @generated by nextrs::bundle. Do not edit by hand.\n\
         // The ONE app-shell: a TanStack Router over every discovered route,\n\
         // mounted once so layout.tsx chrome stays mounted across soft navigation.\n",
    );
    out.push_str("import { createRoot } from \"react-dom/client\";\n");
    out.push_str("import { QueryClient, QueryClientProvider } from \"@tanstack/react-query\";\n");
    out.push_str(
        "import { createRootRoute, createRoute, createRouter, lazyRouteComponent, Outlet, RouterProvider, useParams } from \"@tanstack/react-router\";\n",
    );
    let _ = writeln!(
        out,
        "import {{ seedQueryClient }} from {:?};",
        client_helper.display().to_string()
    );
    for (i, l) in layouts.iter().enumerate() {
        let p = l.layout.tsx.as_ref().unwrap().display().to_string();
        let _ = writeln!(out, "import Layout{i} from {p:?};");
    }
    out.push('\n');

    // Outer QueryClient + seed hydration — identical to the not-found entry_wrapper.
    out.push_str(
        "const qc = new QueryClient({ defaultOptions: { queries: { staleTime: 30_000 } } });\n\
         seedQueryClient(qc);\n\n",
    );
    // Leaf factory: lazy page component fed the router's live matched params
    // as its `params` prop — same page API as a hard load, but it updates on
    // soft navigation (the server's __nx_params__ tag can't).
    out.push_str(
        "function nxLeaf(load: () => Promise<any>) {\n\
           const Lazy = lazyRouteComponent(load);\n\
           return function NxPage() {\n\
             const params = useParams({ strict: false });\n\
             return <Lazy params={params} />;\n\
           };\n\
         }\n\n",
    );
    // Soft-nav data prefetch: routes with a prefetch.rs get a loader that
    // asks the server for the same entries a hard load would stream
    // (GET /__nx/prefetch?path=...), hydrating the query cache. With
    // preload:"intent" this runs on hover, so by click time the leaf paints
    // seeded. Bounded: a slow prefetch aborts at 1s and the page falls back
    // to fetch-on-mount. The very first loader run is the document we just
    // hard-loaded — its seeds are already in the cache, skip the request.
    out.push_str(
        "// The document we hard-loaded already carried its seeds — skip one\n\
         // redundant self-prefetch for it. In-flight prefetches are shared, so\n\
         // hover and the click-time loader await the SAME request.\n\
         const nxInitialHref = window.location.pathname + window.location.search;\n\
         let nxFirstLoad = true;\n\
         const nxInflight = new Map<string, Promise<void>>();\n\
         function nxPrefetch(href: string): Promise<void> {\n\
           if (nxFirstLoad && href === nxInitialHref) {\n\
             nxFirstLoad = false;\n\
             return Promise.resolve();\n\
           }\n\
           let p = nxInflight.get(href);\n\
           if (!p) {\n\
             p = nxFetchSeeds(href).finally(() => {\n\
               setTimeout(() => nxInflight.delete(href), 3000);\n\
             });\n\
             nxInflight.set(href, p);\n\
           }\n\
           return p;\n\
         }\n\
         async function nxFetchSeeds(href: string) {\n\
           try {\n\
             const res = await fetch(\"/__nx/prefetch?path=\" + encodeURIComponent(href), {\n\
               signal: AbortSignal.timeout(1000),\n\
             });\n\
             if (!res.ok) return;\n\
             for (const e of await res.json()) {\n\
               const state = qc.getQueryState(e.key);\n\
               // Don't clobber data newer than the staleTime window.\n\
               if (state && !state.isInvalidated && Date.now() - state.dataUpdatedAt < 30_000) continue;\n\
               qc.setQueryData(e.key, { data: e.data, status: 200, headers: new Headers() });\n\
             }\n\
           } catch {}\n\
         }\n\n",
    );
    out.push_str(
        "const rootRoute = createRootRoute({\n  component: () => (<QueryClientProvider client={qc}><Outlet /></QueryClientProvider>),\n});\n\n",
    );

    // Pathless layout routes (id, not path) — supply mounted chrome only.
    for (i, l) in layouts.iter().enumerate() {
        let parent = parent_ident_of(&layouts, &l.url_path, true);
        let id = page_slug(&l.url_path);
        let _ = writeln!(
            out,
            "const layout_{i} = createRoute({{ getParentRoute: () => {parent}, id: {id:?}, component: () => (<Layout{i}><Outlet /></Layout{i}>) }});"
        );
    }
    out.push('\n');

    // Leaf routes — one per page.tsx at its full path, lazily loaded.
    for (i, pg) in pages.iter().enumerate() {
        let parent = parent_ident_of(&layouts, &pg.url_path, false);
        let path = tanstack_path(&pg.url_path);
        let page_path = pg.page.tsx.as_ref().unwrap().display().to_string();
        let _ = write!(
            out,
            "const route_{i} = createRoute({{ getParentRoute: () => {parent}, path: {path:?}, component: nxLeaf(() => import({page_path:?}))"
        );
        if pg.props.is_some() {
            let _ = write!(
                out,
                ", loader: ({{ location }}: {{ location: {{ href: string }} }}) => nxPrefetch(location.href)"
            );
        }
        if let Some(loading) =
            nearest_tsx_loading_route(routes, &pg.url_path).and_then(|r| r.loading.tsx.as_ref())
        {
            let lp = loading.display().to_string();
            let _ = write!(
                out,
                ", pendingComponent: lazyRouteComponent(() => import({lp:?}))"
            );
        }
        out.push_str(" });\n");
    }
    out.push('\n');

    // Assemble: parent ident -> ordered child idents, then emit recursively.
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    for (i, l) in layouts.iter().enumerate() {
        children
            .entry(parent_ident_of(&layouts, &l.url_path, true))
            .or_default()
            .push(format!("layout_{i}"));
    }
    for (i, pg) in pages.iter().enumerate() {
        children
            .entry(parent_ident_of(&layouts, &pg.url_path, false))
            .or_default()
            .push(format!("route_{i}"));
    }
    let _ = writeln!(
        out,
        "const routeTree = {};\n",
        emit_route_node("rootRoute", &children)
    );

    out.push_str(
        "const router = createRouter({\n  routeTree,\n  defaultPreload: \"intent\",\n  defaultPendingMs: 150,\n  // A path absent from this tree (also absent from the axum page routes — same\n  // discovery source) hard-loads so the server (real route / static / strangler)\n  // handles it. No reload loop.\n  defaultNotFoundComponent: () => {\n    if (typeof window !== \"undefined\") window.location.assign(window.location.href);\n    return null;\n  },\n});\n\n",
    );
    out.push_str("Object.assign(window, { __nx_router__: router });\n\n");

    // Transparent soft navigation: pages and layouts use plain <a> tags (zero
    // app-page changes), so intercept same-origin clicks on URLs this router
    // can render and turn them into router navigations. Anything else — other
    // origins, downloads, modified clicks, new tabs, non-app paths (static
    // files, API links, strangler routes) — falls through to a normal
    // document load. `data-no-soft` opts an anchor out explicitly.
    let route_patterns = pages
        .iter()
        .map(|pg| route_regex(&pg.url_path))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(out, "const NX_APP_ROUTES = [{route_patterns}];");
    out.push_str(
        "function nxAppUrl(e: Event): URL | null {\n\
           const a = e.target instanceof Element ? e.target.closest(\"a\") : null;\n\
           if (!a || (a.target && a.target !== \"_self\")) return null;\n\
           if (a.hasAttribute(\"download\") || a.hasAttribute(\"data-no-soft\")) return null;\n\
           const href = a.getAttribute(\"href\");\n\
           if (!href || href.startsWith(\"#\")) return null;\n\
           const url = new URL(a.href, window.location.href);\n\
           if (url.origin !== window.location.origin) return null;\n\
           if (!NX_APP_ROUTES.some((r) => r.test(url.pathname))) return null;\n\
           return url;\n\
         }\n\
         document.addEventListener(\"click\", (e) => {\n\
           if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;\n\
           const url = nxAppUrl(e);\n\
           if (!url) return;\n\
           e.preventDefault();\n\
           router.navigate({ href: url.pathname + url.search + url.hash });\n\
         });\n\
         // Hover = intent: plain anchors have no TanStack <Link> machinery, so\n\
         // defaultPreload:\"intent\" never sees them — warm the DATA here (the\n\
         // in-flight map in nxPrefetch dedups against the loader at click time).\n\
         document.addEventListener(\"mouseover\", (e) => {\n\
           const url = nxAppUrl(e);\n\
           if (url) nxPrefetch(url.pathname + url.search);\n\
         });\n\n",
    );

    out.push_str(
        "createRoot(document.getElementById(\"__nx_root__\")!).render(<RouterProvider router={router} />);\n",
    );
    // Note: the real "load the next page's React ahead of time" is TanStack's
    // defaultPreload:"intent" above (preloads the target route's chunk on hover).
    // The server's document-level speculation rules (now Prefetch, not Prerender)
    // are a light bonus for hard loads and are left in place.
    out
}

fn loading_entry_wrapper(loading_path: &Path) -> String {
    format!(
        r#"// @generated by nextrs::bundle. Do not edit by hand.
import {{ createRoot }} from "react-dom/client";
import Loading from "{loading}";

createRoot(document.getElementById("__nx_loading_root__")!).render(<Loading />);
"#,
        loading = loading_path.display(),
    )
}

/// Build rolldown's resolve-alias list: the client barrel (a file target,
/// exact match), the built-in `@/*` → `<client_dir>/src/*` (so shadcn-style
/// subpath imports resolve — a directory target can do subpaths, a file
/// target can't), then any user aliases (replacements resolved relative to
/// `client_dir`).
fn build_aliases(
    client_dir: &Path,
    client_alias: &str,
    user_aliases: &[(&str, &str)],
) -> Vec<(String, Vec<Option<String>>)> {
    // The resolver (oxc_resolver) supports `*` wildcards natively: a key
    // containing `*` compiles to AliasMatchKind::Wildcard and the matched
    // segment is substituted into the `*` in the value (compile_alias +
    // load_alias_value). So pass tsconfig-style `X/*` → `dir/*` through with
    // `*` intact in BOTH key and value; non-wildcard patterns map straight
    // through. (Earlier code stripped `*` and appended a trailing slash, which
    // produced a Prefix-kind key like `X/` that never matches `X/sub` —
    // `strip_prefix("X/")` leaves `sub`, which doesn't start with `/` — so
    // every `@workspace/ui/*`, `@workspace/common/*`, `@/*` subpath leaked out
    // as a bare specifier. dashboard-rs zero-copy reuse depends on this.)
    fn norm(pattern: &str, replacement: String) -> (String, Vec<Option<String>>) {
        (pattern.to_string(), vec![Some(replacement)])
    }

    // User aliases first: the first matching key wins, so a user `@/*` entry
    // overrides the built-in `@/*` → `<client_dir>/src/*` default.
    let mut aliases: Vec<(String, Vec<Option<String>>)> = user_aliases
        .iter()
        .map(|(pattern, replacement)| {
            norm(pattern, client_dir.join(replacement).display().to_string())
        })
        .collect();
    aliases.push((
        client_alias.to_string(),
        vec![Some(client_dir.join("src/index.ts").display().to_string())],
    ));
    let builtin = norm("@/*", client_dir.join("src/*").display().to_string());
    if !aliases.iter().any(|(k, _)| *k == builtin.0) {
        aliases.push(builtin);
    }
    aliases
}

fn run_bundler(
    inputs: Vec<rolldown::InputItem>,
    staging: &Path,
    client_dir: &Path,
    client_alias: &str,
    user_aliases: &[(&str, &str)],
) -> std::io::Result<BTreeMap<String, String>> {
    use rolldown::{
        BundlerOptions, CodeSplittingMode, OutputFormat, Platform, PreserveEntrySignatures,
        RawMinifyOptions,
    };

    let release = std::env::var("PROFILE").is_ok_and(|p| p == "release");

    let node_env = if release {
        "\"production\""
    } else {
        "\"development\""
    };

    let options = BundlerOptions {
        input: Some(inputs),
        cwd: Some(client_dir.to_path_buf()),
        dir: Some(staging.display().to_string()),
        format: Some(OutputFormat::Esm),
        platform: Some(Platform::Browser),
        entry_filenames: Some("[name]-[hash].js".to_string().into()),
        chunk_filenames: Some("chunks/[name]-[hash].js".to_string().into()),
        // Each page.tsx is both a named entry and a dynamic import() target from
        // the app-shell; split so each leaf is its own loadable chunk, and don't
        // preserve entry signatures (avoids a per-page facade chunk = extra
        // request — the dynamic import targets the entry chunk directly).
        code_splitting: Some(CodeSplittingMode::Bool(true)),
        preserve_entry_signatures: Some(PreserveEntrySignatures::False),
        minify: Some(RawMinifyOptions::Bool(release)),
        // The concrete map type (FxIndexMap) isn't re-exported by rolldown;
        // let FromIterator name it for us.
        define: Some(
            std::iter::once(("process.env.NODE_ENV".to_string(), node_env.to_string())).collect(),
        ),
        resolve: Some(rolldown::ResolveOptions {
            alias: Some(build_aliases(client_dir, client_alias, user_aliases)),
            modules: Some(vec![client_dir.join("node_modules").display().to_string()]),
            // Prefix-alias substitution (`@/*`, `@workspace/x/*`) yields
            // extension-less paths (e.g. `.../src/errors`); without explicit TS
            // extensions the resolver can't find `errors.ts`, so the specifier
            // leaks out as a bare import and the browser fails to resolve it.
            // List .ts/.tsx first so zero-copy .tsx pages + workspace subpaths
            // resolve. (dashboard-rs aliases regression, by drew's request.)
            extensions: Some(vec![
                ".ts".into(),
                ".tsx".into(),
                ".mjs".into(),
                ".js".into(),
                ".jsx".into(),
                ".json".into(),
                ".cjs".into(),
            ]),
            ..Default::default()
        }),
        // Pin the JSX transform to the automatic runtime. Rolldown discovers
        // each file's nearest tsconfig.json and merges its compilerOptions
        // into the transform; zero-copy setups bundle .tsx files owned by a
        // foreign Next.js tsconfig whose `"jsx": "preserve"` would otherwise
        // disable JSX lowering for exactly those files and emit raw JSX into
        // the chunk (a syntax error at load time in the browser). An explicit
        // runtime wins the merge — rolldown then only warns about the conflict.
        transform: Some(rolldown::BundlerTransformOptions {
            jsx: Some(rolldown::Either::Right(rolldown::JsxOptions {
                runtime: Some("automatic".to_string()),
                ..Default::default()
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    // BundlerBuilder (not Bundler::new) so we can register the SVG-component
    // loader: SVGR-style `.svg` imports in zero-copy React pages would otherwise
    // be parsed as JS and fail. See SvgComponentPlugin below.
    let mut bundler = rolldown::BundlerBuilder::default()
        .with_options(options)
        .with_plugins(vec![std::sync::Arc::new(SvgComponentPlugin)])
        .build()
        .map_err(|e| std::io::Error::other(format!("rolldown: {e:?}")))?;
    let rt = tokio::runtime::Runtime::new()?;
    let output = rt
        .block_on(bundler.write())
        .map_err(|e| std::io::Error::other(format!("rolldown bundling failed: {e:?}")))?;

    // Rolldown ERRORS on unresolved relative imports but only WARNS on
    // unresolved bare specifiers — it externalizes them, leaving a literal
    // `import ... from "pkg"` in the emitted module. Browsers can't resolve
    // bare specifiers, so that's a guaranteed runtime TypeError on every page
    // that loads the chunk (how the docs site shipped a dead landing page:
    // @tanstack/react-router missing from the client's package.json).
    // Promote those warnings to build failures with the actionable fix.
    let unresolved: Vec<String> = output
        .warnings
        .iter()
        .map(|w| format!("{w:?}"))
        .filter(|w| w.contains("UNRESOLVED_IMPORT"))
        .collect();
    if !unresolved.is_empty() {
        return Err(std::io::Error::other(format!(
            "nextrs: bundling left unresolved bare imports (a runtime error in \
             the browser). Usually a missing dependency — add it to {}/package.json \
             and `npm install`. Details: {}",
            client_dir.display(),
            unresolved.join("; ")
        )));
    }
    for w in &output.warnings {
        println!("cargo:warning=nextrs bundle: {w:?}");
    }
    let entries = output
        .assets
        .iter()
        .filter_map(|asset| match asset {
            rolldown_common::Output::Chunk(chunk) if chunk.is_entry => {
                Some((chunk.name.to_string(), format!("/dist/{}", chunk.filename)))
            }
            _ => None,
        })
        .collect();
    Ok(entries)
}

/// Mirror `src` into `dst`, writing a file only when its bytes differ
/// (temp-file-then-rename so concurrent identical builds can't tear), and
/// pruning anything in `dst` that `src` no longer has.
fn mirror_by_content(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::collections::HashSet;

    let mut seen: HashSet<std::ffi::OsString> = HashSet::new();
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        seen.insert(name.clone());
        let from = entry.path();
        let to = dst.join(&name);
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&to)?;
            mirror_by_content(&from, &to)?;
        } else {
            let bytes = std::fs::read(&from)?;
            write_if_changed(&to, &bytes)?;
        }
    }

    for entry in std::fs::read_dir(dst)? {
        let entry = entry?;
        if seen.contains(&entry.file_name()) {
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

fn write_if_changed(path: &Path, content: &[u8]) -> std::io::Result<()> {
    if let Ok(existing) = std::fs::read(path) {
        if existing == content {
            return Ok(());
        }
    }
    let tmp = path.with_extension("nextrs-tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_slug_shapes() {
        assert_eq!(page_slug("/"), "index");
        assert_eq!(page_slug("/todos"), "todos");
        assert_eq!(page_slug("/users/{id}"), "users-_id_");
        assert_eq!(page_slug("/a/b/c"), "a-b-c");
    }

    #[test]
    fn aliases_include_barrel_and_shadcn_default() {
        let aliases = build_aliases(Path::new("/proj/client"), "@site/client", &[]);
        // The client barrel maps to the index file (exact match).
        assert!(
            aliases.iter().any(|(k, v)| k == "@site/client"
                && v[0].as_deref() == Some("/proj/client/src/index.ts")),
            "{aliases:?}"
        );
        // Built-in shadcn-style @/* → <client>/src/*, with `*` kept intact in
        // key and value — oxc_resolver supports wildcards natively and
        // substitutes the matched segment (the old prefix normalization never
        // matched subpaths, leaking them as bare specifiers).
        assert!(
            aliases
                .iter()
                .any(|(k, v)| k == "@/*" && v[0].as_deref() == Some("/proj/client/src/*")),
            "{aliases:?}"
        );
    }

    #[test]
    fn user_aliases_resolve_relative_to_client_dir() {
        let aliases = build_aliases(
            Path::new("/proj/client"),
            "@site/client",
            &[("~/*", "vendor/*")],
        );
        assert!(
            aliases
                .iter()
                .any(|(k, v)| k == "~/*" && v[0].as_deref() == Some("/proj/client/vendor/*")),
            "{aliases:?}"
        );
    }

    #[test]
    fn user_alias_overrides_builtin_shadcn_default() {
        let aliases = build_aliases(
            Path::new("/proj/client"),
            "@site/client",
            &[("@/*", "../src/*")],
        );
        let at_entries: Vec<_> = aliases.iter().filter(|(k, _)| k == "@/*").collect();
        assert_eq!(at_entries.len(), 1, "{aliases:?}");
        assert_eq!(
            at_entries[0].1[0].as_deref(),
            Some("/proj/client/../src/*"),
            "{aliases:?}"
        );
        // And the user entry must come before any built-in would (first match wins).
        assert_eq!(aliases[0].0, "@/*", "{aliases:?}");
    }

    #[test]
    fn exact_aliases_pass_through_unnormalized() {
        let aliases = build_aliases(
            Path::new("/proj/client"),
            "@site/client",
            &[("react-dom/client", "vendor/react-dom-client.ts")],
        );
        assert!(
            aliases.iter().any(|(k, v)| k == "react-dom/client"
                && v[0].as_deref() == Some("/proj/client/vendor/react-dom-client.ts")),
            "{aliases:?}"
        );
    }

    #[test]
    fn bundle_config_default_allows_partial_construction() {
        // Default + ..Default::default() keeps new fields additive/non-breaking.
        let cfg = BundleConfig {
            app_dir: "app",
            ..Default::default()
        };
        assert_eq!(cfg.app_dir, "app");
        assert!(cfg.aliases.is_empty());
    }

    #[test]
    fn write_if_changed_skips_identical() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.js");
        write_if_changed(&p, b"same").unwrap();
        let mtime1 = std::fs::metadata(&p).unwrap().modified().unwrap();
        write_if_changed(&p, b"same").unwrap();
        assert_eq!(mtime1, std::fs::metadata(&p).unwrap().modified().unwrap());
        write_if_changed(&p, b"diff").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"diff");
    }

    #[test]
    fn stylesheet_is_copied_under_a_content_addressed_name() {
        let tmp = tempfile::tempdir().unwrap();
        let public = tmp.path().join("public");
        let staging = tmp.path().join("staging");
        std::fs::create_dir_all(&public).unwrap();
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(public.join("style.css"), b"body { color: tomato; }").unwrap();

        let href = fingerprint_stylesheet(tmp.path(), &staging)
            .unwrap()
            .expect("stylesheet href");
        let expected = format!(
            "/dist/style-{}.css",
            crate::build::content_hash(b"body { color: tomato; }")
        );

        assert_eq!(href, expected);
        assert_eq!(
            std::fs::read(staging.join(href.trim_start_matches("/dist/"))).unwrap(),
            b"body { color: tomato; }"
        );
    }

    #[test]
    fn skipped_first_build_uses_compilable_asset_placeholders() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("page.tsx"), "export default function Page() {}").unwrap();
        let routes = discover_routes(&app);

        let manifest =
            manifest_from_existing_dist(&routes, &tmp.path().join("public/dist"), tmp.path())
                .unwrap();

        assert_eq!(
            manifest.entries.get("__app_shell__").map(String::as_str),
            Some("/dist/__app_shell__.js")
        );
        assert_eq!(
            manifest.entries.get("index").map(String::as_str),
            Some("/dist/index.js")
        );
    }

    #[test]
    fn mirror_prunes_and_copies() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(src.join("chunks")).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(src.join("a.js"), "a").unwrap();
        std::fs::write(src.join("chunks/shared.js"), "s").unwrap();
        std::fs::write(dst.join("stale.js"), "old").unwrap();

        mirror_by_content(&src, &dst).unwrap();
        assert_eq!(std::fs::read(dst.join("a.js")).unwrap(), b"a");
        assert_eq!(std::fs::read(dst.join("chunks/shared.js")).unwrap(), b"s");
        assert!(!dst.join("stale.js").exists());
    }

    #[test]
    fn app_shell_entry_mounts_router_and_provider() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("page.tsx"), "").unwrap();
        let routes = discover_routes(tmp.path());
        let s = app_shell_entry(&routes, Path::new("/abs/helper.ts"));
        assert!(s.contains("__nx_root__"));
        assert!(s.contains("createRootRoute"));
        assert!(s.contains("createRouter"));
        assert!(s.contains("RouterProvider"));
        assert!(s.contains("QueryClientProvider"));
        assert!(s.contains("seedQueryClient"));
        assert!(s.contains("__nx_router__"));
        assert!(s.contains("import { seedQueryClient } from \"/abs/helper.ts\";"));
    }

    #[test]
    fn entry_wrapper_composes_layouts_root_to_leaf() {
        let s = entry_wrapper(
            Path::new("/abs/app/dashboard/page.tsx"),
            &[
                PathBuf::from("/abs/app/layout.tsx"),
                PathBuf::from("/abs/app/dashboard/layout.tsx"),
            ],
            Path::new("/abs/client.ts"),
        );
        assert!(s.contains("import Layout0 from \"/abs/app/layout.tsx\";"));
        assert!(s.contains("import Layout1 from \"/abs/app/dashboard/layout.tsx\";"));
        assert!(s.contains("<Layout0><Layout1><Page params={params} /></Layout1></Layout0>"));
    }

    #[test]
    fn entry_wrapper_reads_route_params() {
        let s = entry_wrapper(Path::new("/abs/page.tsx"), &[], Path::new("/abs/helper.ts"));
        assert!(s.contains("__nx_params__"));
        assert!(s.contains("<Page params={params} />"));
    }

    const URL_HOOKS_SPEC: &str = r#"{
      "paths": {
        "/api/todos": {
          "get": {
            "operationId": "getTodos",
            "tags": ["todos"],
            "parameters": [
              { "name": "status", "in": "query", "schema": { "type": "string" } },
              { "name": "page", "in": "query", "schema": { "type": "integer" } }
            ]
          },
          "post": { "operationId": "addTodo", "tags": ["todos"] }
        },
        "/api/todos/{id}": {
          "get": {
            "operationId": "getApiTodosById",
            "tags": ["todos"],
            "parameters": [
              { "name": "id", "in": "path", "schema": { "type": "integer" } }
            ]
          }
        },
        "/api/tracks/{id}/plays": {
          "get": {
            "operationId": "getTrackPlays",
            "tags": ["tracks"],
            "parameters": [
              { "name": "id", "in": "path", "schema": { "type": "integer" } },
              { "name": "range", "in": "query", "schema": { "type": "string" } }
            ]
          }
        },
        "/api/ping": {
          "get": { "operationId": "getApiPing", "tags": ["ping"] }
        }
      }
    }"#;

    #[test]
    fn url_hook_ops_selects_gets_with_query_params() {
        let ops = url_hook_ops(URL_HOOKS_SPEC);
        // Path-only and no-param GETs (and POSTs) get no URL-bound variant;
        // path+query routes DO (path params become explicit hook arguments).
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].hook, "useGetTodos");
        assert_eq!(ops[0].tag, "todos");
        assert!(ops[0].path_keys.is_empty());
        assert_eq!(ops[0].query_keys, ["status", "page"]);
        assert_eq!(ops[1].hook, "useGetTrackPlays");
        assert_eq!(ops[1].path_keys, ["id"]);
        assert_eq!(ops[1].query_keys, ["range"]);
    }

    #[test]
    fn url_hooks_source_path_params_are_explicit_leading_args() {
        // The bug this guards: orval's hook signature for a path+query route
        // is useX(id, params?) — params sits at index 1, and the wrapper must
        // pass the path value through, not spread params onto it.
        let src = url_hooks_source(&url_hook_ops(URL_HOOKS_SPEC));
        assert!(src.contains(
            "export function useGetTrackPlaysFromUrl(id: Parameters<typeof useGetTrackPlays>[0], opts?: {"
        ), "{src}");
        assert!(
            src.contains(
                "type useGetTrackPlaysParams = NonNullable<Parameters<typeof useGetTrackPlays>[1]>;"
            ),
            "{src}"
        );
        assert!(
            src.contains("const query = useGetTrackPlays(id, params);"),
            "{src}"
        );
        // Query-only hooks keep the argument-0 shape.
        assert!(
            src.contains(
                "type useGetTodosParams = NonNullable<Parameters<typeof useGetTodos>[0]>;"
            ),
            "{src}"
        );
        assert!(src.contains("const query = useGetTodos(params);"), "{src}");
    }

    #[test]
    fn url_hooks_source_shape() {
        let src = url_hooks_source(&url_hook_ops(URL_HOOKS_SPEC));
        assert!(src.contains(r#"import { useGetTodos } from "./todos/todos";"#));
        assert!(src.contains("export function useGetTodosFromUrl"));
        // Types derive from the orval hook — no guessed type names.
        assert!(src.contains("NonNullable<Parameters<typeof useGetTodos>[0]>"));
        // Params read the live URL, only the declared keys.
        assert!(src.contains(r#"nxPick(search, ["status", "page"])"#));
        // Setter is a soft navigation.
        assert!(src.contains("const setParams"));
        assert!(src.contains("useNavigate"));
    }

    #[test]
    fn emit_url_hooks_writes_and_removes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src/generated/todos")).unwrap();
        std::fs::write(tmp.path().join("openapi.json"), URL_HOOKS_SPEC).unwrap();
        emit_url_hooks(tmp.path()).unwrap();
        let hooks = tmp.path().join("src/generated/url-hooks.ts");
        assert!(hooks.is_file());

        // Barrel picks it up.
        emit_client_barrel(tmp.path()).unwrap();
        let barrel = std::fs::read_to_string(tmp.path().join("src/generated/index.ts")).unwrap();
        assert!(barrel.contains(r#"export * from "./url-hooks";"#));

        // Spec loses its eligible ops → stale file is removed.
        std::fs::write(tmp.path().join("openapi.json"), r#"{"paths":{}}"#).unwrap();
        emit_url_hooks(tmp.path()).unwrap();
        assert!(!hooks.exists());
    }

    #[test]
    fn emit_url_hooks_skips_ops_whose_tag_module_is_missing() {
        // A torn generated dir (interrupted npm run gen) must not produce a
        // wrapper importing a module that isn't there.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src/generated/model")).unwrap();
        std::fs::write(tmp.path().join("openapi.json"), URL_HOOKS_SPEC).unwrap();
        emit_url_hooks(tmp.path()).unwrap();
        assert!(!tmp.path().join("src/generated/url-hooks.ts").exists());
    }

    #[test]
    fn client_barrel_exports_every_tag_and_model() {
        let tmp = tempfile::tempdir().unwrap();
        let generated = tmp.path().join("src/generated");
        for dir in ["todos", "ping", "model"] {
            std::fs::create_dir_all(generated.join(dir)).unwrap();
        }
        emit_client_barrel(tmp.path()).unwrap();
        let barrel = std::fs::read_to_string(generated.join("index.ts")).unwrap();
        assert!(barrel.contains(r#"export * from "./ping/ping";"#));
        assert!(barrel.contains(r#"export * from "./todos/todos";"#));
        assert!(barrel.contains(r#"export * from "./model";"#));
        // model is not a tag module.
        assert!(!barrel.contains("./model/model"));
    }

    #[test]
    fn client_barrel_is_a_noop_without_generated_client() {
        let tmp = tempfile::tempdir().unwrap();
        emit_client_barrel(tmp.path()).unwrap();
        assert!(!tmp.path().join("src/generated/index.ts").exists());
    }

    #[test]
    fn route_regex_shapes() {
        assert_eq!(route_regex("/"), r"/^\/$/");
        assert_eq!(route_regex("/todos"), r"/^\/todos$/");
        assert_eq!(route_regex("/source/{id}"), r"/^\/source\/[^\/]+$/");
        assert_eq!(route_regex("/files/{*rest}"), r"/^\/files\/.+$/");
        assert_eq!(route_regex("/(group)/about"), r"/^\/about$/");
        // Literal dots escaped: /foo.bar must not match /fooxbar.
        assert_eq!(route_regex("/foo.bar"), r"/^\/foo\.bar$/");
    }

    #[test]
    fn app_shell_intercepts_same_origin_anchor_clicks() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("source/[id]")).unwrap();
        std::fs::write(tmp.path().join("page.tsx"), "").unwrap();
        std::fs::write(tmp.path().join("source/[id]/page.tsx"), "").unwrap();
        let routes = discover_routes(tmp.path());
        let s = app_shell_entry(&routes, Path::new("/abs/helper.ts"));
        // Plain <a> clicks soft-navigate when the router owns the URL...
        assert!(s.contains("document.addEventListener(\"click\""));
        assert!(s.contains("router.navigate({ href:"));
        // ...matching against the discovered route set, params included.
        assert!(s.contains(r"const NX_APP_ROUTES = [/^\/$/, /^\/source\/[^\/]+$/];"));
        // Escape hatches: modified clicks, downloads, and explicit opt-out.
        assert!(s.contains("data-no-soft"));
        assert!(s.contains("e.metaKey"));
    }

    #[test]
    fn app_shell_prefetch_loader_only_on_prefetch_backed_routes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("todos/[id]")).unwrap();
        std::fs::create_dir_all(tmp.path().join("about")).unwrap();
        std::fs::write(tmp.path().join("todos/[id]/page.tsx"), "").unwrap();
        std::fs::write(tmp.path().join("todos/[id]/prefetch.rs"), "").unwrap();
        std::fs::write(tmp.path().join("about/page.tsx"), "").unwrap();
        let routes = discover_routes(tmp.path());
        let s = app_shell_entry(&routes, Path::new("/abs/helper.ts"));

        // Soft-nav prefetch: the seeded route gets a loader hitting the
        // endpoint; the plain route doesn't.
        assert!(s.contains("function nxPrefetch"));
        assert!(s.contains("/__nx/prefetch?path="));
        // Exactly one loader (the prefetch-backed leaf).
        assert_eq!(s.matches("loader: ({ location }").count(), 1, "{s}");
        // Hydration respects freshness and matches the seed envelope.
        assert!(s.contains(
            "qc.setQueryData(e.key, { data: e.data, status: 200, headers: new Headers() })"
        ));
        assert!(s.contains("AbortSignal.timeout(1000)"));
    }

    #[test]
    fn app_shell_leaves_receive_live_router_params() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("source/[id]")).unwrap();
        std::fs::write(tmp.path().join("source/[id]/page.tsx"), "").unwrap();
        let routes = discover_routes(tmp.path());
        let s = app_shell_entry(&routes, Path::new("/abs/helper.ts"));
        // Leaves render through nxLeaf, which feeds TanStack's live params in
        // as the `params` prop (the __nx_params__ tag goes stale on soft nav).
        assert!(s.contains("function nxLeaf"));
        assert!(s.contains("useParams({ strict: false })"));
        assert!(s.contains("<Lazy params={params} />"));
        assert!(s.contains("component: nxLeaf(() => import("));
    }

    #[test]
    fn app_shell_entry_nests_pages_under_layout_and_maps_dynamic_segments() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("orgs/[slug]/billing")).unwrap();
        std::fs::write(tmp.path().join("orgs/[slug]/layout.tsx"), "").unwrap();
        std::fs::write(tmp.path().join("orgs/[slug]/billing/page.tsx"), "").unwrap();
        let routes = discover_routes(tmp.path());
        let s = app_shell_entry(&routes, Path::new("/abs/helper.ts"));
        // pathless layout route (id, not path), parented to root
        assert!(s.contains("const layout_0 = createRoute({ getParentRoute: () => rootRoute, id:"));
        // leaf at its full path with [slug] -> $slug, parented to the layout route
        assert!(s.contains("path: \"/orgs/$slug/billing\""));
        assert!(s.contains("getParentRoute: () => layout_0"));
        // tree assembly nests the layout's children
        assert!(s.contains("layout_0.addChildren("));
    }

    #[test]
    fn loading_entry_wrapper_mounts_loading_component() {
        let s = loading_entry_wrapper(Path::new("/abs/app/loading.tsx"));
        assert!(s.contains("__nx_loading_root__"));
        assert!(s.contains("import Loading from \"/abs/app/loading.tsx\";"));
    }

    #[test]
    fn page_bundles_emit_slug_and_path() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("dashboard/settings")).unwrap();
        std::fs::write(tmp.path().join("dashboard/settings/page.tsx"), "").unwrap();

        let routes = discover_routes(tmp.path());
        let pages = page_bundles(&routes);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].slug, "dashboard-settings");
        assert!(pages[0].page_path.ends_with("dashboard/settings/page.tsx"));
    }

    #[test]
    fn not_found_tsx_produces_standalone_bundle_under_not_found_slug() {
        // A segment can carry both a page.tsx and a not-found.tsx. The page is
        // a raw app-shell entry; the not-found is a standalone wrapped mount
        // (it renders outside the router) carrying the segment's tsx layouts.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("admin")).unwrap();
        std::fs::write(tmp.path().join("layout.tsx"), "").unwrap();
        std::fs::write(tmp.path().join("admin/page.tsx"), "").unwrap();
        std::fs::write(tmp.path().join("admin/not-found.tsx"), "").unwrap();

        let routes = discover_routes(tmp.path());

        let pages = page_bundles(&routes);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].slug, "admin");
        assert!(pages[0].page_path.ends_with("admin/page.tsx"));

        let not_founds = not_found_bundles(&routes);
        assert_eq!(not_founds.len(), 1);
        let nf = &not_founds[0];
        assert_eq!(nf.slug, "admin-not-found");
        assert!(nf.page_path.ends_with("admin/not-found.tsx"));
        // not-found is wrapped in the same segment layouts as the page.
        assert_eq!(nf.layout_paths.len(), 1);
        assert!(nf.layout_paths[0].ends_with("layout.tsx"));
    }
}

// --- SVG-as-React-component loader ------------------------------------------
// SVGR-style `.svg` imports compile to a default-exported React component in
// Next apps (e.g. `import Logo from "./logo.svg"; <Logo className=.. />`).
// Rolldown has no SVG loader, so it parses the raw `<svg>` markup as JS and
// fails ("Unexpected JSX expression"). This `load` hook intercepts `.svg` by
// extension and emits a tiny component that inlines the markup via
// dangerouslySetInnerHTML, so zero-copy pages importing `.svg` bundle unchanged.
#[derive(Debug)]
struct SvgComponentPlugin;

impl SvgComponentPlugin {
    async fn load_impl(
        &self,
        args: &rolldown_plugin::HookLoadArgs<'_>,
    ) -> rolldown_plugin::HookLoadReturn {
        let clean = args.id.split(['?', '#']).next().unwrap_or(args.id);
        let ext = std::path::Path::new(clean)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let code = match ext.as_deref() {
            Some("svg") => {
                // SVGR-style component: inline the markup via dangerouslySetInnerHTML.
                let svg = tokio::fs::read_to_string(clean).await?;
                let lit = js_string_literal(&svg);
                format!(
                    "import {{ jsx }} from \"react/jsx-runtime\";\n\
                     const __HTML = {lit};\n\
                     export default function SvgAsset(props) {{\n  \
                     return jsx(\"span\", {{ ...props, dangerouslySetInnerHTML: {{ __html: __HTML }} }});\n}}\n"
                )
            }
            Some("css") => {
                // rolldown dropped CSS bundling, so a `import \"x.css\"` side-effect
                // import would hard-fail. Emit a JS module that injects the stylesheet
                // at runtime — styles apply, build succeeds.
                let css = tokio::fs::read_to_string(clean).await?;
                let lit = js_string_literal(&css);
                format!(
                    "const __CSS = {lit};\n\
                     if (typeof document !== \"undefined\") {{\n  \
                     const s = document.createElement(\"style\");\n  \
                     s.setAttribute(\"data-nextrs-css\", \"1\");\n  \
                     s.textContent = __CSS;\n  \
                     document.head.appendChild(s);\n}}\n\
                     export default {{}};\n"
                )
            }
            _ => return Ok(None),
        };
        Ok(Some(rolldown_plugin::HookLoadOutput {
            code: code.into(),
            module_type: Some(rolldown_common::ModuleType::Js),
            ..Default::default()
        }))
    }
}

impl rolldown_plugin::Plugin for SvgComponentPlugin {
    fn name(&self) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("nextrs:asset-loader")
    }

    fn register_hook_usage(&self) -> rolldown_plugin::HookUsage {
        rolldown_plugin::HookUsage::Load | rolldown_plugin::HookUsage::Transform
    }

    fn load_meta(&self) -> Option<rolldown_plugin::PluginHookMeta> {
        // Resolve `.svg` before the builtin asset/default loaders.
        Some(rolldown_plugin::PluginHookMeta {
            order: Some(rolldown_plugin::PluginOrder::Pre),
        })
    }

    fn load(
        &self,
        _ctx: rolldown_plugin::SharedLoadPluginContext,
        args: &rolldown_plugin::HookLoadArgs<'_>,
    ) -> impl std::future::Future<Output = rolldown_plugin::HookLoadReturn> + Send {
        self.load_impl(args)
    }

    fn transform(
        &self,
        _ctx: rolldown_plugin::SharedTransformPluginContext,
        args: &rolldown_plugin::HookTransformArgs<'_>,
    ) -> impl std::future::Future<Output = rolldown_plugin::HookTransformReturn> + Send {
        // A `"use server"` module (Next.js server action) would otherwise pull the
        // whole server stack (auth/prisma/node) into the browser bundle. Next swaps
        // these for network stubs; we emit client stubs so the page bundles + renders.
        let out = server_action_stub(args.code);
        async move {
            Ok(out.map(|code| rolldown_plugin::HookTransformOutput {
                code: Some(code),
                module_type: Some(rolldown_common::ModuleType::Js),
                ..Default::default()
            }))
        }
    }
}

/// If `code` is a `"use server"` module, return a stub module that re-exports the
/// same names as no-op client functions (each returns a next-safe-action-shaped
/// `{ serverError }`), so the server import chain never reaches the browser.
fn server_action_stub(code: &str) -> Option<String> {
    let head = code.trim_start();
    let is_server = head.starts_with("\"use server\"") || head.starts_with("'use server'");
    if !is_server {
        return None;
    }
    let mut names: Vec<String> = Vec::new();
    let mut has_default = false;
    let ident: fn(&str) -> String = |s| {
        s.chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
            .collect()
    };
    for line in code.lines() {
        let l = line.trim_start();
        if l.starts_with("export default") {
            has_default = true;
        } else if let Some(rest) = l
            .strip_prefix("export const ")
            .or_else(|| l.strip_prefix("export let "))
            .or_else(|| l.strip_prefix("export var "))
        {
            let n = ident(rest);
            if !n.is_empty() {
                names.push(n);
            }
        } else if let Some(rest) = l
            .strip_prefix("export async function ")
            .or_else(|| l.strip_prefix("export function "))
        {
            let n = ident(rest);
            if !n.is_empty() {
                names.push(n);
            }
        } else if let Some(rest) = l.strip_prefix("export {") {
            let inner = rest.split('}').next().unwrap_or("");
            for part in inner.split(',') {
                let p = part.trim();
                let n = p.rsplit(" as ").next().unwrap_or(p).trim();
                if !n.is_empty() && n != "default" {
                    names.push(n.to_string());
                }
            }
        }
    }
    let mut out = String::new();
    out.push_str(
        "// nextrs: \"use server\" module stubbed for the browser bundle (no action runtime).\n\
         const __action = (name) => async () => ({ data: undefined, serverError: \
         \"Server action \" + name + \" is not available in the nextrs port.\" });\n",
    );
    for n in &names {
        out.push_str(&format!("export const {n} = __action(\"{n}\");\n"));
    }
    if has_default {
        out.push_str("export default __action(\"default\");\n");
    }
    Some(out)
}

/// Minimal JS double-quoted string literal for arbitrary text (the SVG markup).
fn js_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
