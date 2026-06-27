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

pub use crate::build::{loading_slug, page_slug};
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
    let mut inputs = Vec::with_capacity(tsx_pages.len() + tsx_loadings.len() + 1);

    // The ONE app-shell entry: a TanStack Router built from every discovered
    // route, mounted once. Every page.tsx document boots /dist/__app_shell__.js
    // (see build::tsx_page_shell), so shared layout.tsx chrome stays mounted
    // across soft navigation and only the changed leaf swaps.
    let shell_path = entries_dir.join("__app_shell__.tsx");
    write_if_changed(&shell_path, app_shell_entry(&routes, &client_helper).as_bytes())?;
    inputs.push(rolldown::InputItem {
        name: Some("__app_shell__".to_string()),
        import: shell_path.display().to_string(),
    });
    // Each page.tsx is ALSO a named entry (the RAW page, no createRoot wrapper) so
    // it gets a stable /dist/<slug>.js that the app-shell's lazy
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
}

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

/// url_path → `layout_<i>` ident for a route that owns a `layout.tsx`.
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

/// The single app-shell entry: a TanStack Router built from every discovered
/// route, mounted ONCE into `#__nx_root__`. Each `layout.tsx` becomes a pathless
/// layout route rendering the layout around an `<Outlet/>` (so it stays mounted
/// across soft navigation); each `page.tsx` becomes a lazily-loaded leaf at its
/// full path. Replaces the per-page `entry_wrapper`.
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
    let mut pages: Vec<&DiscoveredRoute> =
        routes.iter().filter(|r| r.page.tsx.is_some()).collect();
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
        "import { createRootRoute, createRoute, createRouter, lazyRouteComponent, Outlet, RouterProvider } from \"@tanstack/react-router\";\n",
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

    // Outer QueryClient + seed hydration — identical to the retired entry_wrapper.
    out.push_str(
        "const qc = new QueryClient({ defaultOptions: { queries: { staleTime: 30_000 } } });\n\
         seedQueryClient(qc);\n\n",
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
            "const route_{i} = createRoute({{ getParentRoute: () => {parent}, path: {path:?}, component: lazyRouteComponent(() => import({page_path:?}))"
        );
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
    let _ = writeln!(out, "const routeTree = {};\n", emit_route_node("rootRoute", &children));

    out.push_str(
        "const router = createRouter({\n  routeTree,\n  defaultPreload: \"intent\",\n  defaultPendingMs: 150,\n  // A path absent from this tree (also absent from the axum page routes — same\n  // discovery source) hard-loads so the server (real route / static / strangler)\n  // handles it. No reload loop.\n  defaultNotFoundComponent: () => {\n    if (typeof window !== \"undefined\") window.location.assign(window.location.href);\n    return null;\n  },\n});\n\n",
    );
    out.push_str("Object.assign(window, { __nx_router__: router });\n\n");
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
) -> std::io::Result<()> {
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
        entry_filenames: Some("[name].js".to_string().into()),
        chunk_filenames: Some("chunks/[name].js".to_string().into()),
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
