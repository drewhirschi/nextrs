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
//! Bundle names are stable (`/dist/<slug>.js`, shared chunks under
//! `/dist/chunks/`) — no content hashing yet; revisit if CDN staleness bites.

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

/// Discover `page.tsx` routes, bundle them, and mirror the output into
/// `<public_dist>`. No-op when the app has no `.tsx` pages.
pub fn bundle_pages(cfg: &BundleConfig) -> std::io::Result<()> {
    // Escape hatch for the client-generation bootstrap: a brand-new page.tsx
    // may import hooks that `npm run gen` hasn't generated yet, while `npm run
    // gen` itself needs `cargo build` (for dump-openapi). The dump script sets
    // NEXTRS_SKIP_BUNDLE=1 to break the cycle.
    println!("cargo:rerun-if-env-changed=NEXTRS_SKIP_BUNDLE");
    if std::env::var_os("NEXTRS_SKIP_BUNDLE").is_some_and(|v| v == "1") {
        return Ok(());
    }

    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set in build.rs"),
    );
    let abs_app = manifest_dir.join(cfg.app_dir).canonicalize()?;
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

    let routes = discover_routes(&abs_app);
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

    let dist = manifest_dir.join(cfg.public_dist);
    if tsx_pages.is_empty() && tsx_loadings.is_empty() {
        // Prune a stale dist from a previous build that had tsx pages.
        if dist.is_dir() {
            std::fs::remove_dir_all(&dist)?;
        }
        return Ok(());
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
    let mut inputs = Vec::with_capacity(tsx_pages.len() + tsx_loadings.len());
    for page in &tsx_pages {
        let slug = &page.slug;
        let entry_path = entries_dir.join(format!("{}.tsx", slug));
        let entry_src = entry_wrapper(&page.page_path, &page.layout_paths, &client_helper);
        write_if_changed(&entry_path, entry_src.as_bytes())?;
        inputs.push(rolldown::InputItem {
            name: Some(slug.clone()),
            import: entry_path.display().to_string(),
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

    let staging = out_dir.join("nextrs_dist");
    if staging.is_dir() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;

    run_bundler(inputs, &staging, &client_dir, cfg.client_alias, cfg.aliases)?;

    std::fs::create_dir_all(&dist)?;
    mirror_by_content(&staging, &dist)
}

#[derive(Debug, Clone)]
struct PageBundle {
    slug: String,
    page_path: PathBuf,
    layout_paths: Vec<PathBuf>,
}

/// Browser bundles for every client-rendered entry: each `page.tsx` and each
/// `not-found.tsx`. Both compose the segment's `.tsx` layouts client-side and
/// mount into `__nx_root__`, so they share [`entry_wrapper`]; they only differ
/// in slug ([`page_slug`] vs [`not_found_slug`]), which keeps a segment's page
/// and not-found bundles from colliding.
fn page_bundles(routes: &[DiscoveredRoute]) -> Vec<PageBundle> {
    routes
        .iter()
        .flat_map(|route| {
            let layout_paths = collect_layouts_for_path(routes, &route.url_path);
            let page = route.page.tsx.clone().map(|page_path| PageBundle {
                slug: page_slug(&route.url_path),
                page_path,
                layout_paths: layout_paths.clone(),
            });
            let not_found = route.not_found.tsx.clone().map(|page_path| PageBundle {
                slug: not_found_slug(&route.url_path),
                page_path,
                layout_paths: layout_paths.clone(),
            });
            page.into_iter().chain(not_found)
        })
        .collect()
}

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

// staleTime > 0 so server-seeded entries (see props.rs) render without an
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
) -> std::io::Result<()> {
    use rolldown::{BundlerOptions, OutputFormat, Platform, RawMinifyOptions};

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
        entry_filenames: Some("[name].js".to_string().into()),
        chunk_filenames: Some("chunks/[name].js".to_string().into()),
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
    rt.block_on(bundler.write())
        .map_err(|e| std::io::Error::other(format!("rolldown bundling failed: {e:?}")))?;
    Ok(())
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
        // Built-in shadcn-style @/* → <client>/src/*, normalized to the
        // resolver's prefix form (`*` is not a glob to the resolver — alias
        // keys are webpack-style prefix matches).
        assert!(
            aliases
                .iter()
                .any(|(k, v)| k == "@/" && v[0].as_deref() == Some("/proj/client/src/")),
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
                .any(|(k, v)| k == "~/" && v[0].as_deref() == Some("/proj/client/vendor/")),
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
        let at_entries: Vec<_> = aliases.iter().filter(|(k, _)| k == "@/").collect();
        assert_eq!(at_entries.len(), 1, "{aliases:?}");
        assert_eq!(
            at_entries[0].1[0].as_deref(),
            Some("/proj/client/../src/"),
            "{aliases:?}"
        );
        // And the user entry must come before any built-in would (first match wins).
        assert_eq!(aliases[0].0, "@/", "{aliases:?}");
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
    fn entry_wrapper_mentions_mount_and_provider() {
        let s = entry_wrapper(Path::new("/abs/page.tsx"), &[], Path::new("/abs/helper.ts"));
        assert!(s.contains("__nx_root__"));
        assert!(s.contains("QueryClientProvider"));
        assert!(s.contains("seedQueryClient"));
        assert!(s.contains("/abs/page.tsx"));
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

    #[test]
    fn loading_entry_wrapper_mounts_loading_component() {
        let s = loading_entry_wrapper(Path::new("/abs/app/loading.tsx"));
        assert!(s.contains("__nx_loading_root__"));
        assert!(s.contains("import Loading from \"/abs/app/loading.tsx\";"));
    }

    #[test]
    fn page_bundles_include_applicable_layouts() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("dashboard/settings")).unwrap();
        std::fs::write(tmp.path().join("layout.tsx"), "").unwrap();
        std::fs::write(tmp.path().join("dashboard/layout.tsx"), "").unwrap();
        std::fs::write(tmp.path().join("dashboard/settings/page.tsx"), "").unwrap();

        let routes = discover_routes(tmp.path());
        let pages = page_bundles(&routes);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].slug, "dashboard-settings");
        assert_eq!(pages[0].layout_paths.len(), 2);
        assert!(pages[0].layout_paths[0].ends_with("layout.tsx"));
        assert!(pages[0].layout_paths[1].ends_with("dashboard/layout.tsx"));
    }

    #[test]
    fn not_found_tsx_produces_bundle_under_not_found_slug() {
        // A segment can carry both a page.tsx and a not-found.tsx; each gets its
        // own bundle, under page_slug / not_found_slug respectively, and both
        // pick up the segment's tsx layouts.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("admin")).unwrap();
        std::fs::write(tmp.path().join("layout.tsx"), "").unwrap();
        std::fs::write(tmp.path().join("admin/page.tsx"), "").unwrap();
        std::fs::write(tmp.path().join("admin/not-found.tsx"), "").unwrap();

        let routes = discover_routes(tmp.path());
        let bundles = page_bundles(&routes);

        let page = bundles
            .iter()
            .find(|b| b.slug == "admin")
            .expect("page.tsx bundle");
        assert!(page.page_path.ends_with("admin/page.tsx"));
        assert_eq!(page.layout_paths.len(), 1);

        let nf = bundles
            .iter()
            .find(|b| b.slug == "admin-not-found")
            .expect("not-found.tsx bundle");
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
        rolldown_plugin::HookUsage::Load
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
