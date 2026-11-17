//! Preload effectiveness telemetry: is preloading earning its keep in this
//! app? **Off by default; opt in with `NEXTRS_PRELOAD_STATS=1`.**
//!
//! The app shell preloads a link's route chunk when the link is on screen and
//! its `prefetch.rs` data on intent — hover, or a prediction (pointer
//! trajectory, Tab, scroll, touch). For every soft navigation the shell
//! measures how long the destination took to load and how much head start
//! hover and the predictors each had before the click.
//!
//! With the switch on, the router:
//!
//! - emits `<meta name="nx-preload-stats">` in every React page document,
//!   which is what tells the shell to report at all, and
//! - mounts `POST /__nx/preload-stats`, which turns each report into `tracing`
//!   events (target `nextrs::preload`) — the same escalation path as route
//!   telemetry: stdout / Vercel function logs, an OTLP exporter, or any
//!   subscriber that captures events into a store.
//!
//! Two events:
//!
//! - **`preload navigation`**, one per soft navigation: `http.route` (the
//!   route template), `trigger`, `predictor`, `data`, `load_ms`,
//!   `lead_hover_ms`, `lead_predicted_ms`, and the derived waits the user
//!   would have felt under each strategy — `wait_none_ms`, `wait_hover_ms`,
//!   `wait_predictive_ms` — plus `saved_ms`, what prediction bought over
//!   hover alone.
//! - **`preload cost`**, one per report: preloads fired and preloads followed
//!   by a click, for hover and for the predictors.
//!
//! Only route templates and timings are accepted — never a URL or a param
//! value. The endpoint is unauthenticated (it is called from every visitor's
//! browser), so it treats the body as hostile: size-capped, shape-checked,
//! and every string checked against a fixed alphabet before it is logged.

use std::sync::OnceLock;

use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use serde::Deserialize;

/// Where the app shell POSTs its reports when preload stats are enabled.
pub const NX_PRELOAD_STATS_PATH: &str = "/__nx/preload-stats";

/// Emitted in every React page document when enabled; its presence is what
/// switches the shell's reporting on.
pub(crate) const SHELL_TAG: &str =
    r#"<meta name="nx-preload-stats" content="/__nx/preload-stats">"#;

const MAX_BODY_BYTES: usize = 32 * 1024;
const MAX_NAVS: usize = 50;
/// No real preload or lead time is a day long; anything bigger is junk.
const MAX_MS: f64 = 86_400_000.0;

/// On only when `NEXTRS_PRELOAD_STATS` opts in (`1`, `true`, `on`). Checked
/// once per process.
pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| opted_in(std::env::var("NEXTRS_PRELOAD_STATS").ok().as_deref()))
}

fn opted_in(value: Option<&str>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "on")
    )
}

#[derive(Debug, Deserialize)]
struct Report {
    #[serde(default)]
    navs: Vec<Nav>,
    #[serde(default)]
    fired: Counts,
    #[serde(default)]
    used: Counts,
}

#[derive(Debug, Default, Deserialize)]
struct Counts {
    #[serde(default)]
    hover: u32,
    #[serde(default)]
    predicted: u32,
}

#[derive(Debug, Deserialize)]
struct Nav {
    route: String,
    #[serde(default)]
    data: bool,
    trigger: String,
    load_ms: f64,
    lead_hover_ms: Option<f64>,
    lead_predicted_ms: Option<f64>,
    predictor: Option<String>,
}

/// The waits a navigation would have cost under each strategy, in ms.
#[derive(Debug, PartialEq)]
struct Waits {
    none: f64,
    hover: f64,
    predictive: f64,
}

impl Nav {
    fn valid(&self) -> bool {
        route_like(&self.route)
            && signal_like(&self.trigger)
            && self.predictor.as_deref().is_none_or(signal_like)
            && ms_like(self.load_ms)
            && self.lead_hover_ms.is_none_or(ms_like)
            && self.lead_predicted_ms.is_none_or(ms_like)
    }

    /// No preload: the whole load. Hover: the load minus hover's head start.
    /// Predictive (hover AND the predictors): minus the earlier of the two.
    fn waits(&self) -> Waits {
        let hover_lead = self.lead_hover_ms.unwrap_or(0.0);
        let best_lead = hover_lead.max(self.lead_predicted_ms.unwrap_or(0.0));
        Waits {
            none: self.load_ms,
            hover: (self.load_ms - hover_lead).max(0.0),
            predictive: (self.load_ms - best_lead).max(0.0),
        }
    }
}

fn ms_like(ms: f64) -> bool {
    ms.is_finite() && (0.0..=MAX_MS).contains(&ms)
}

/// A route template: `/todos/{id}`, `/(app)/docs/{*rest}`.
fn route_like(s: &str) -> bool {
    s.starts_with('/')
        && s.len() <= 200
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_-.~{}*()[]$".contains(&b))
}

/// A signal name: `hover`, `none`, `mouse/trajectory`, `tab/forwards`.
fn signal_like(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 40
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_-".contains(&b))
}

fn parse(body: &[u8]) -> Option<Report> {
    let report: Report = serde_json::from_slice(body).ok()?;
    (report.navs.len() <= MAX_NAVS && report.navs.iter().all(Nav::valid)).then_some(report)
}

/// `POST /__nx/preload-stats`. Always answers without a body: `204` when the
/// report was accepted, `400` when it was not. Sent by `navigator.sendBeacon`,
/// so nothing reads the response anyway.
pub(crate) async fn handler(req: Request<Body>) -> StatusCode {
    let Ok(body) = axum::body::to_bytes(req.into_body(), MAX_BODY_BYTES).await else {
        return StatusCode::PAYLOAD_TOO_LARGE;
    };
    let Some(report) = parse(&body) else {
        return StatusCode::BAD_REQUEST;
    };
    for nav in &report.navs {
        let waits = nav.waits();
        tracing::info!(
            target: "nextrs::preload",
            {
                http.route = %nav.route,
                trigger = %nav.trigger,
                predictor = nav.predictor.as_deref(),
                data = nav.data,
                load_ms = nav.load_ms,
                lead_hover_ms = nav.lead_hover_ms,
                lead_predicted_ms = nav.lead_predicted_ms,
                wait_none_ms = waits.none,
                wait_hover_ms = waits.hover,
                wait_predictive_ms = waits.predictive,
                saved_ms = waits.hover - waits.predictive,
            },
            "preload navigation"
        );
    }
    tracing::info!(
        target: "nextrs::preload",
        {
            navigations = report.navs.len(),
            hover_fired = report.fired.hover,
            hover_used = report.used.hover,
            predicted_fired = report.fired.predicted,
            predicted_used = report.used.predicted,
        },
        "preload cost"
    );
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nav(load: f64, hover: Option<f64>, predicted: Option<f64>) -> Nav {
        Nav {
            route: "/todos/{id}".into(),
            data: true,
            trigger: "mouse/trajectory".into(),
            load_ms: load,
            lead_hover_ms: hover,
            lead_predicted_ms: predicted,
            predictor: Some("mouse/trajectory".into()),
        }
    }

    #[test]
    fn off_unless_explicitly_opted_in() {
        assert!(!opted_in(None));
        assert!(!opted_in(Some("")));
        assert!(!opted_in(Some("0")));
        assert!(opted_in(Some("1")));
        assert!(opted_in(Some(" TRUE ")));
        assert!(opted_in(Some("on")));
    }

    #[test]
    fn waits_compare_no_preload_hover_and_predictive() {
        // 300ms load; hover got there 180ms before the click, the trajectory
        // predictor 280ms before: hover alone leaves 120ms, predictive 20ms.
        let w = nav(300.0, Some(180.0), Some(280.0)).waits();
        assert_eq!(
            w,
            Waits {
                none: 300.0,
                hover: 120.0,
                predictive: 20.0
            }
        );
        // A head start longer than the load is simply instant.
        assert_eq!(nav(50.0, Some(400.0), None).waits().hover, 0.0);
        // Keyboard: no hover ever happened, so hover-only saves nothing.
        let w = nav(200.0, None, Some(150.0)).waits();
        assert_eq!(
            w,
            Waits {
                none: 200.0,
                hover: 200.0,
                predictive: 50.0
            }
        );
        // Predictive includes hover: a late predictor never makes it worse.
        assert_eq!(
            nav(300.0, Some(180.0), Some(20.0)).waits().predictive,
            120.0
        );
    }

    #[test]
    fn accepts_a_well_formed_report() {
        let body = br#"{"navs":[{"route":"/todos/{id}","data":true,"trigger":"hover",
            "load_ms":41.5,"lead_hover_ms":210,"lead_predicted_ms":null,"predictor":null}],
            "fired":{"hover":3,"predicted":2},"used":{"hover":1,"predicted":0}}"#;
        let report = parse(body).unwrap();
        assert_eq!(report.navs.len(), 1);
        assert_eq!(report.fired.predicted, 2);
        assert_eq!(report.used.hover, 1);
    }

    #[test]
    fn rejects_anything_that_is_not_a_template_and_timings() {
        let with = |route: &str, trigger: &str, load: &str| {
            format!(
                r#"{{"navs":[{{"route":"{route}","trigger":"{trigger}","load_ms":{load},
                   "lead_hover_ms":null,"lead_predicted_ms":null,"predictor":null}}]}}"#
            )
        };
        assert!(parse(with("/todos/{id}", "hover", "10").as_bytes()).is_some());
        // A real URL with a query string or a log-forging newline is not a template.
        assert!(parse(with("/todos/7?token=abc", "hover", "10").as_bytes()).is_none());
        assert!(parse(with("/a\\nlevel=ERROR", "hover", "10").as_bytes()).is_none());
        assert!(parse(with("todos", "hover", "10").as_bytes()).is_none());
        assert!(parse(with("/todos", "hover now", "10").as_bytes()).is_none());
        assert!(parse(with("/todos", "hover", "-1").as_bytes()).is_none());
        assert!(parse(with("/todos", "hover", "1e300").as_bytes()).is_none());
        assert!(parse(b"not json").is_none());
        // Too many navigations in one report.
        let one = r#"{"route":"/","trigger":"none","load_ms":1,"lead_hover_ms":null,"lead_predicted_ms":null,"predictor":null}"#;
        let many = format!(r#"{{"navs":[{}]}}"#, vec![one; MAX_NAVS + 1].join(","));
        assert!(parse(many.as_bytes()).is_none());
    }
}
