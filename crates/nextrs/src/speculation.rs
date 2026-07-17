//! Speculative navigation — document-level prefetch/prerender via the
//! browser's native [Speculation Rules API]. **Off by default; opt-in.**
//!
//! Not to be confused with nextrs's two other ahead-of-time mechanisms:
//!
//! - **prefetch** (data seeding): a route's `prefetch.rs` and the
//!   `/__nx/prefetch` endpoint warm a React page's query cache.
//! - **preload** (route chunk warming): the app shell's TanStack router
//!   preloads a React route's JS chunk on hover (`router.preloadRoute`).
//!
//! *Speculation* is a third thing: a declarative
//! `<script type="speculationrules">` injected into the `<head>` of
//! full-document responses that lets the browser fetch (or fully render) the
//! *next document* in the background, so a hard navigation is instant.
//!
//! ## Who it's for
//!
//! Server-rendered pages (`page.rs` / `page.html` + `layout.html`) — there is
//! no app shell on those routes, so every `<a>` click is a full-document HTTP
//! GET, and document speculation is what makes it feel instant. It also helps
//! any *hard* navigation (e.g. from a React page into a server-rendered
//! section).
//!
//! React (`page.tsx`) routes don't need it: the app shell intercepts
//! same-origin clicks to them (soft navigation) and already warms both the
//! route chunk (preload) and the data (prefetch) on hover. A speculatively
//! fetched document for those URLs would never be used — which is why, when
//! speculation is enabled, the injected rules **exclude React app-shell
//! routes** (see [`SpeculationConfig::script_excluding`]).
//!
//! ## Enabling it
//!
//! ```ignore
//! use nextrs::{Eagerness, SpeculationConfig, SpeculationMode};
//!
//! let app = nextrs::router::build_router_with_speculation(
//!     generated_registry(),
//!     SpeculationConfig { mode: SpeculationMode::Prefetch, eagerness: Eagerness::Moderate },
//! );
//! ```
//!
//! There is **no JavaScript** — it's a browser-native hint. Browsers without
//! support ignore it and navigate normally.
//!
//! ## Behavior change in 0.4.0
//!
//! Through 0.3.7 this was on by default (as `PrefetchConfig`, injected into
//! every full-document response). Since 0.4.0 the default is
//! [`SpeculationMode::Off`]: for React-routed apps the prefetched documents
//! were pure waste (a full server render per hover, discarded by the soft-nav
//! interceptor), and server-rendered apps opt in deliberately as above.
//!
//! ## Opting a link out
//!
//! A link with `data-no-prefetch` is excluded from the rules (e.g. logout or
//! other destructive GETs you don't want fetched early):
//!
//! ```html
//! <a href="/logout" data-no-prefetch>Log out</a>
//! ```
//!
//! ## Future direction
//!
//! [`SpeculationMode::Prerender`] at a higher [`Eagerness`] gives "prerender
//! basically every page" — the next document is rendered in a hidden tab and
//! activation is instant (it composes well with nextrs streaming: the
//! loading→page swap completes during prerender, so the loading shell never
//! even shows). Page-level opt-out (a page declaring it must not be
//! prerendered, via a `Supports-Loading-Mode` header) is the natural next
//! extension on top of the per-link `data-no-prefetch` already wired here.
//!
//! [Speculation Rules API]: https://developer.mozilla.org/en-US/docs/Web/API/Speculation_Rules_API

/// How aggressively the browser acts on a speculation rule. Maps directly to the
/// Speculation Rules `eagerness` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eagerness {
    /// Act only on a clear signal the user is about to navigate (pointer-down).
    Conservative,
    /// Act on hover / sustained pointer-down — the `<Link>`-like choice.
    Moderate,
    /// Act as soon as a candidate link is reasonably likely (e.g. in viewport).
    Eager,
    /// Act immediately when the rules are parsed, for every matching link.
    Immediate,
}

impl Eagerness {
    fn as_str(self) -> &'static str {
        match self {
            Eagerness::Conservative => "conservative",
            Eagerness::Moderate => "moderate",
            Eagerness::Eager => "eager",
            Eagerness::Immediate => "immediate",
        }
    }
}

/// What the browser should do with a speculated navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeculationMode {
    /// Inject nothing — speculative navigation disabled. The default.
    Off,
    /// Fetch the next document into the prefetch cache (cheap; instant TTFB on
    /// navigation).
    Prefetch,
    /// Fully render the next document in a hidden tab; activation is instant.
    /// Heavier (runs the page early) but the snappiest.
    Prerender,
}

impl SpeculationMode {
    /// The Speculation Rules top-level key (`"prefetch"` / `"prerender"`).
    fn rule_key(self) -> Option<&'static str> {
        match self {
            SpeculationMode::Off => None,
            SpeculationMode::Prefetch => Some("prefetch"),
            SpeculationMode::Prerender => Some("prerender"),
        }
    }
}

/// Document-level speculative-navigation configuration for a router.
///
/// Controls only the injected `<script type="speculationrules">` — it has no
/// effect on data prefetch (`prefetch.rs` / `/__nx/prefetch`) or on the app
/// shell's route-chunk preload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeculationConfig {
    pub mode: SpeculationMode,
    pub eagerness: Eagerness,
}

/// Deprecated name for [`SpeculationConfig`].
#[deprecated(
    note = "renamed to SpeculationConfig — this only controls document-level Speculation Rules, not data prefetch"
)]
pub type PrefetchConfig = SpeculationConfig;

impl Default for SpeculationConfig {
    /// Off — speculation is an opt-in (changed in 0.4.0; see the module docs).
    fn default() -> Self {
        Self::OFF
    }
}

impl SpeculationConfig {
    /// Disable speculative navigation entirely (also the default).
    pub const OFF: Self = Self {
        mode: SpeculationMode::Off,
        eagerness: Eagerness::Moderate,
    };

    /// The `<script type="speculationrules">…</script>` tag to inject, or `None`
    /// when [`SpeculationMode::Off`].
    ///
    /// Rule shape: same-origin links (`href_matches: "/*"`), excluding any link
    /// marked `data-no-prefetch`, at the configured eagerness. The router uses
    /// [`Self::script_excluding`] instead so React app-shell routes are also
    /// excluded.
    pub fn script(&self) -> Option<String> {
        self.script_excluding(&[])
    }

    /// Like [`Self::script`], but the rules additionally exclude links whose
    /// href matches any of `exclude_href_patterns` ([URL Pattern] syntax, e.g.
    /// `"/todos"`, `"/source/:id"`). The router passes the app's React
    /// app-shell routes here: the shell soft-navigates those URLs, so a
    /// speculatively fetched document for them would never be used.
    ///
    /// [URL Pattern]: https://developer.mozilla.org/en-US/docs/Web/API/URL_Pattern_API
    pub fn script_excluding(&self, exclude_href_patterns: &[String]) -> Option<String> {
        let key = self.mode.rule_key()?;
        let mut and = vec![
            serde_json::json!({"href_matches": "/*"}),
            serde_json::json!({"not": {"selector_matches": "[data-no-prefetch]"}}),
        ];
        if !exclude_href_patterns.is_empty() {
            and.push(serde_json::json!({"not": {"href_matches": exclude_href_patterns}}));
        }
        let rules = serde_json::json!({
            key: [{"where": {"and": and}, "eagerness": self.eagerness.as_str()}]
        });
        Some(format!(
            r#"<script type="speculationrules">{rules}</script>"#
        ))
    }

    /// Splice [`Self::script`] in just before the first `</head>` in `before`.
    ///
    /// Returns `before` unchanged when speculation is off, or when there's no
    /// `</head>` — the latter scopes injection to full HTML documents and leaves
    /// head-less fragments (and API responses) untouched.
    pub fn inject_into_head(&self, before: String) -> String {
        inject(self.script().as_deref(), before)
    }

    /// Resolve this config against an app's React app-shell routes (axum path
    /// syntax, e.g. `"/todos"`, `"/source/{id}"`): the final injectable script
    /// with those routes excluded, computed once at router build.
    pub(crate) fn resolve(&self, react_routes: &[String]) -> ResolvedSpeculation {
        let patterns: Vec<String> = react_routes
            .iter()
            .map(|p| href_pattern_for_route(p))
            .collect();
        ResolvedSpeculation {
            script: self.script_excluding(&patterns),
        }
    }
}

/// A [`SpeculationConfig`] resolved against a route table: the precomputed
/// script (if any), injected per response without re-serializing.
#[derive(Debug, Clone, Default)]
pub(crate) struct ResolvedSpeculation {
    script: Option<String>,
}

impl ResolvedSpeculation {
    /// Same contract as [`SpeculationConfig::inject_into_head`].
    pub(crate) fn inject_into_head(&self, before: String) -> String {
        inject(self.script.as_deref(), before)
    }
}

fn inject(script: Option<&str>, before: String) -> String {
    let Some(script) = script else {
        return before;
    };
    if !before.contains("</head>") {
        return before;
    }
    before.replacen("</head>", &format!("{script}</head>"), 1)
}

/// Convert a route path in axum pattern syntax to [URL Pattern] syntax for a
/// Speculation Rules `href_matches` clause: `{id}` → `:id` (one segment),
/// `{*rest}` → `*` (the remainder), `(group)` segments dropped (they never
/// appear in URLs — mirrors `route_regex` in `bundle.rs`).
///
/// [URL Pattern]: https://developer.mozilla.org/en-US/docs/Web/API/URL_Pattern_API
pub(crate) fn href_pattern_for_route(url_path: &str) -> String {
    if url_path == "/" {
        return "/".to_string();
    }
    let mut pattern = String::new();
    for seg in url_path.split('/').filter(|s| !s.is_empty()) {
        if seg.starts_with('(') && seg.ends_with(')') {
            continue;
        }
        pattern.push('/');
        if seg.starts_with("{*") && seg.ends_with('}') {
            pattern.push('*');
        } else if seg.starts_with('{') && seg.ends_with('}') {
            pattern.push(':');
            pattern.push_str(&seg[1..seg.len() - 1]);
        } else {
            pattern.push_str(seg);
        }
    }
    if pattern.is_empty() {
        // Only group segments — the route lives at "/".
        pattern.push('/');
    }
    pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> SpeculationConfig {
        SpeculationConfig {
            mode: SpeculationMode::Prefetch,
            eagerness: Eagerness::Moderate,
        }
    }

    #[test]
    fn default_is_off() {
        // Behavior change in 0.4.0: speculation is opt-in.
        let cfg = SpeculationConfig::default();
        assert_eq!(cfg.mode, SpeculationMode::Off);
        assert!(cfg.script().is_none());
        let html = "<html><head></head><body>x</body></html>".to_string();
        assert_eq!(cfg.inject_into_head(html.clone()), html);
    }

    #[test]
    fn enabled_prefetch_moderate_script_shape() {
        let script = on().script().expect("enabled config emits a script");
        assert!(script.contains(r#"type="speculationrules""#));
        assert!(script.contains(r#""prefetch""#));
        assert!(script.contains(r#""eagerness":"moderate""#));
        assert!(script.contains(r#""href_matches":"/*""#));
        // Per-link opt-out is wired from day one.
        assert!(script.contains(r#""selector_matches":"[data-no-prefetch]""#));
        assert!(!script.contains(r#""prerender""#));
        // No exclusion clause when nothing is excluded.
        assert!(!script.contains(r#""not":{"href_matches""#));
    }

    #[test]
    fn off_emits_no_script_and_no_injection() {
        assert!(SpeculationConfig::OFF.script().is_none());
        let html = "<html><head></head><body>x</body></html>".to_string();
        assert_eq!(SpeculationConfig::OFF.inject_into_head(html.clone()), html);
    }

    #[test]
    fn prerender_mode_uses_prerender_key() {
        let cfg = SpeculationConfig {
            mode: SpeculationMode::Prerender,
            eagerness: Eagerness::Eager,
        };
        let script = cfg.script().unwrap();
        assert!(script.contains(r#""prerender""#));
        assert!(script.contains(r#""eagerness":"eager""#));
        assert!(!script.contains(r#""prefetch""#));
    }

    #[test]
    fn script_excluding_adds_a_not_href_matches_clause() {
        let excl = vec!["/".to_string(), "/source/:id".to_string()];
        let script = on().script_excluding(&excl).unwrap();
        assert!(script.contains(r#""not":{"href_matches":["/","/source/:id"]}"#));
        // The broad match and per-link opt-out are still present.
        assert!(script.contains(r#""href_matches":"/*""#));
        assert!(script.contains(r#""selector_matches":"[data-no-prefetch]""#));
    }

    #[test]
    fn script_excluding_off_still_emits_nothing() {
        assert!(
            SpeculationConfig::OFF
                .script_excluding(&["/".to_string()])
                .is_none()
        );
    }

    #[test]
    fn injects_immediately_before_head_close() {
        let before = "<html><head><title>t</title></head><body><main>".to_string();
        let out = on().inject_into_head(before);
        let script_at = out.find("speculationrules").unwrap();
        let head_close = out.find("</head>").unwrap();
        let body_at = out.find("<body>").unwrap();
        assert!(script_at < head_close, "script lands inside <head>");
        assert!(head_close < body_at);
    }

    #[test]
    fn headless_fragment_is_untouched() {
        // No </head> → a fragment (or API body); leave it alone.
        let fragment = r#"<div class="root"><h1>Home</h1></div>"#.to_string();
        assert_eq!(on().inject_into_head(fragment.clone()), fragment);
    }

    #[test]
    fn only_first_head_close_is_targeted() {
        let before = "<head>a</head><head>b</head>".to_string();
        let out = on().inject_into_head(before);
        assert_eq!(out.matches("speculationrules").count(), 1);
        // Spliced into the first head, not the second.
        let first = out.find("</head>").unwrap();
        assert!(out.find("speculationrules").unwrap() < first);
    }

    #[test]
    fn href_pattern_conversion() {
        assert_eq!(href_pattern_for_route("/"), "/");
        assert_eq!(href_pattern_for_route("/todos"), "/todos");
        assert_eq!(href_pattern_for_route("/source/{id}"), "/source/:id");
        assert_eq!(href_pattern_for_route("/files/{*rest}"), "/files/*");
        assert_eq!(href_pattern_for_route("/(group)/about"), "/about");
        assert_eq!(href_pattern_for_route("/(group)"), "/");
    }

    #[test]
    fn resolve_converts_route_paths_and_precomputes_the_script() {
        let resolved = on().resolve(&["/".to_string(), "/source/{id}".to_string()]);
        let html = "<head></head>".to_string();
        let out = resolved.inject_into_head(html);
        assert!(out.contains(r#""not":{"href_matches":["/","/source/:id"]}"#));
    }

    // The old names must keep compiling for one release (with deprecation
    // warnings, which this test deliberately does not silence).
    #[test]
    fn deprecated_prefetch_config_alias_still_works() {
        let cfg = PrefetchConfig::OFF;
        assert!(cfg.script().is_none());
        assert_eq!(PrefetchConfig::default(), SpeculationConfig::default());
    }
}
