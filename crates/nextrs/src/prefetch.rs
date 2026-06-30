//! Speculative navigation — automatic prefetch/prerender via the browser's
//! native [Speculation Rules API].
//!
//! nextrs has no client router: every `<a>` click is a full-document HTTP GET.
//! So "prefetch" here doesn't mean warming a JS data cache (what Next.js does) —
//! it means letting the browser fetch (or fully render) the *next document* in
//! the background so the navigation is instant.
//!
//! The framework injects a single declarative `<script type="speculationrules">`
//! into the `<head>` of every full-document page response. There is **no
//! JavaScript** — it's a browser-native hint, so it stays inside the "no client
//! framework, no JS bundle" non-goal. Browsers without support (Safari/Firefox
//! today) ignore it and navigate normally.
//!
//! ## Defaults
//!
//! On by default: [`SpeculationMode::Prefetch`] at [`Eagerness::Moderate`] —
//! same-origin links are prefetched on hover / pointer-down, which is the
//! Next.js-`<Link>` feel. Override with [`crate::router::build_router_with_prefetch`].
//!
//! ## Opting out
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
//! loading→page swap completes during prerender, so the loading shell never even
//! shows). Page-level opt-out (a page declaring it must not be prerendered, via
//! a `Supports-Loading-Mode` header) is the natural next extension on top of the
//! per-link `data-no-prefetch` already wired here.
//!
//! [Speculation Rules API]: https://developer.mozilla.org/en-US/docs/Web/API/Speculation_Rules_API

/// How aggressively the browser acts on a speculation rule. Maps directly to the
/// Speculation Rules `eagerness` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eagerness {
    /// Act only on a clear signal the user is about to navigate (pointer-down).
    Conservative,
    /// Act on hover / sustained pointer-down — the `<Link>`-like default.
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
    /// Inject nothing — speculative navigation disabled.
    Off,
    /// Fetch the next document into the prefetch cache (cheap; instant TTFB on
    /// navigation). The default.
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

/// Speculative-navigation configuration for a router.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefetchConfig {
    pub mode: SpeculationMode,
    pub eagerness: Eagerness,
}

impl Default for PrefetchConfig {
    /// Prefetch same-origin links on hover — on by default.
    fn default() -> Self {
        Self {
            mode: SpeculationMode::Prefetch,
            eagerness: Eagerness::Moderate,
        }
    }
}

impl PrefetchConfig {
    /// Disable speculative navigation entirely.
    pub const OFF: Self = Self {
        mode: SpeculationMode::Off,
        eagerness: Eagerness::Moderate,
    };

    /// The `<script type="speculationrules">…</script>` tag to inject, or `None`
    /// when [`SpeculationMode::Off`].
    ///
    /// Rule shape: same-origin links (`href_matches: "/*"`), excluding any link
    /// marked `data-no-prefetch`, at the configured eagerness.
    pub fn script(&self) -> Option<String> {
        let key = self.mode.rule_key()?;
        Some(format!(
            concat!(
                r#"<script type="speculationrules">"#,
                r#"{{"{key}":[{{"where":{{"and":[{{"href_matches":"/*"}},"#,
                r#"{{"not":{{"selector_matches":"[data-no-prefetch]"}}}}]}},"#,
                r#""eagerness":"{eagerness}"}}]}}"#,
                r#"</script>"#,
            ),
            key = key,
            eagerness = self.eagerness.as_str(),
        ))
    }

    /// Splice [`Self::script`] in just before the first `</head>` in `before`.
    ///
    /// Returns `before` unchanged when speculation is off, or when there's no
    /// `</head>` — the latter scopes injection to full HTML documents and leaves
    /// head-less fragments (and API responses) untouched.
    pub fn inject_into_head(&self, before: String) -> String {
        let Some(script) = self.script() else {
            return before;
        };
        if !before.contains("</head>") {
            return before;
        }
        before.replacen("</head>", &format!("{script}</head>"), 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_prefetch_moderate() {
        let cfg = PrefetchConfig::default();
        assert_eq!(cfg.mode, SpeculationMode::Prefetch);
        assert_eq!(cfg.eagerness, Eagerness::Moderate);
        let script = cfg.script().expect("default emits a script");
        assert!(script.contains(r#"type="speculationrules""#));
        assert!(script.contains(r#""prefetch""#));
        assert!(script.contains(r#""eagerness":"moderate""#));
        assert!(script.contains(r#""href_matches":"/*""#));
        // Per-link opt-out is wired from day one.
        assert!(script.contains(r#""selector_matches":"[data-no-prefetch]""#));
        assert!(!script.contains(r#""prerender""#));
    }

    #[test]
    fn off_emits_no_script_and_no_injection() {
        assert!(PrefetchConfig::OFF.script().is_none());
        let html = "<html><head></head><body>x</body></html>".to_string();
        assert_eq!(PrefetchConfig::OFF.inject_into_head(html.clone()), html);
    }

    #[test]
    fn prerender_mode_uses_prerender_key() {
        let cfg = PrefetchConfig {
            mode: SpeculationMode::Prerender,
            eagerness: Eagerness::Eager,
        };
        let script = cfg.script().unwrap();
        assert!(script.contains(r#""prerender""#));
        assert!(script.contains(r#""eagerness":"eager""#));
        assert!(!script.contains(r#""prefetch""#));
    }

    #[test]
    fn injects_immediately_before_head_close() {
        let before = "<html><head><title>t</title></head><body><main>".to_string();
        let out = PrefetchConfig::default().inject_into_head(before);
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
        assert_eq!(
            PrefetchConfig::default().inject_into_head(fragment.clone()),
            fragment,
        );
    }

    #[test]
    fn only_first_head_close_is_targeted() {
        let before = "<head>a</head><head>b</head>".to_string();
        let out = PrefetchConfig::default().inject_into_head(before);
        assert_eq!(out.matches("speculationrules").count(), 1);
        // Spliced into the first head, not the second.
        let first = out.find("</head>").unwrap();
        assert!(out.find("speculationrules").unwrap() < first);
    }
}
