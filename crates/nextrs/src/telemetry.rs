//! Per-route latency telemetry: who was slow, and why.
//!
//! Every matched route records a timing breakdown — middleware chain, handler
//! (page render or API fn), and named sub-segments like the `seed` step of a
//! prefetch-backed React page. The data surfaces three ways, progressively:
//!
//! 1. **`Server-Timing` response header** (default on): open the browser's
//!    Network tab and read the breakdown per request. Kill switch:
//!    `NEXTRS_SERVER_TIMING=0`.
//! 2. **[`Timing`] extractor**: handlers add their own segments —
//!    `timing.span("db", fetch_todos()).await` — and they appear in the same
//!    header and events on the next reload.
//! 3. **`tracing`**: a `nextrs.route` span wraps middleware + handler, and a
//!    summary event (target `nextrs::telemetry`) fires per request with
//!    OpenTelemetry semantic-convention fields. Local dev sees it with
//!    `RUST_LOG=nextrs=debug`; production ships it wherever the app's
//!    subscriber points (stdout → Vercel function logs, or an OTLP exporter
//!    via `tracing-opentelemetry`).
//!
//! Streaming pages (`loading` present) send headers before the page fn runs,
//! so their `Server-Timing` carries only the middleware segment and a
//! `streaming` marker; the full breakdown still reaches `tracing` when the
//! stream completes.

use std::borrow::Cow;
use std::convert::Infallible;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::Response;

/// Shared per-request timing record. Created by the framework's route
/// handlers, carried through request and response extensions, read by the
/// router-wide [`record`] layer. `Arc`'d because streaming responses finish
/// after the response object is gone.
pub struct RouteTelemetry {
    route: Arc<str>,
    method: http::Method,
    start: Instant,
    mw: OnceLock<Duration>,
    handler: OnceLock<Duration>,
    status: OnceLock<u16>,
    cold: OnceLock<bool>,
    segments: Mutex<Vec<Segment>>,
    streaming: AtomicBool,
    emitted: AtomicBool,
}

pub(crate) type Handle = Arc<RouteTelemetry>;

struct Segment {
    name: Cow<'static, str>,
    dur: Duration,
}

impl RouteTelemetry {
    pub(crate) fn new(route: &str, method: http::Method) -> Handle {
        Arc::new(Self {
            route: Arc::from(route),
            method,
            start: Instant::now(),
            mw: OnceLock::new(),
            handler: OnceLock::new(),
            status: OnceLock::new(),
            cold: OnceLock::new(),
            segments: Mutex::new(Vec::new()),
            streaming: AtomicBool::new(false),
            emitted: AtomicBool::new(false),
        })
    }

    pub(crate) fn record_mw(&self, dur: Duration) {
        let _ = self.mw.set(dur);
    }

    pub(crate) fn record_handler(&self, dur: Duration) {
        let _ = self.handler.set(dur);
    }

    pub(crate) fn set_status(&self, status: u16) {
        let _ = self.status.set(status);
    }

    pub(crate) fn set_cold(&self, cold: bool) {
        let _ = self.cold.set(cold);
    }

    pub(crate) fn set_streaming(&self) {
        self.streaming.store(true, Ordering::Relaxed);
    }

    pub(crate) fn is_streaming(&self) -> bool {
        self.streaming.load(Ordering::Relaxed)
    }

    pub(crate) fn mark(&self, name: impl Into<Cow<'static, str>>, dur: Duration) {
        if let Ok(mut segments) = self.segments.lock() {
            segments.push(Segment {
                name: name.into(),
                dur,
            });
        }
    }

    pub(crate) fn mw_ms(&self) -> Option<f64> {
        self.mw.get().map(|d| ms(*d))
    }

    pub(crate) fn handler_ms(&self) -> Option<f64> {
        self.handler.get().map(|d| ms(*d))
    }

    /// Named segments as `(name, ms)` pairs, in recording order.
    pub(crate) fn segments_ms(&self) -> Vec<(Cow<'static, str>, f64)> {
        self.segments
            .lock()
            .map(|s| s.iter().map(|seg| (seg.name.clone(), ms(seg.dur))).collect())
            .unwrap_or_default()
    }

    pub(crate) fn route(&self) -> &str {
        &self.route
    }

    /// Emit the per-request summary event. Idempotent: the non-streaming path
    /// emits from the [`record`] layer, the streaming path from the stream's
    /// final chunk, and [`Drop`] backstops streams a client abandoned.
    pub(crate) fn emit(&self) {
        if self.emitted.swap(true, Ordering::Relaxed) {
            return;
        }
        let segments = self
            .segments_ms()
            .iter()
            .map(|(name, ms)| format!("{name}={ms:.1}"))
            .collect::<Vec<_>>()
            .join(",");
        let seed_ms = self
            .segments_ms()
            .iter()
            .find(|(name, _)| name == "seed")
            .map(|(_, ms)| *ms);
        let route = &*self.route;
        let method = self.method.as_str();
        let status = self.status.get().copied().unwrap_or(0);
        let total_ms = ms(self.start.elapsed());
        let mw_ms = self.mw_ms().unwrap_or(0.0);
        let handler_ms = self.handler_ms();
        let streaming = self.is_streaming();
        let cold = self.cold.get().copied().unwrap_or(false);
        tracing::info!(
            target: "nextrs::telemetry",
            {
                http.route = route,
                http.request.method = method,
                http.response.status_code = status,
                total_ms,
                mw_ms,
                handler_ms,
                seed_ms,
                segments = %segments,
                streaming,
                cold,
            },
            "request"
        );
    }
}

impl Drop for RouteTelemetry {
    fn drop(&mut self) {
        // Last Arc gone without an emit — an abandoned stream. Still worth a
        // summary: the durations recorded so far are real.
        self.emit();
    }
}

fn ms(dur: Duration) -> f64 {
    dur.as_secs_f64() * 1000.0
}

/// Time a scope and record it as a named segment when the guard drops.
/// Returned by [`start_segment`]; a no-op when the request carries no
/// telemetry handle.
pub struct SegmentGuard {
    handle: Option<Handle>,
    name: &'static str,
    start: Instant,
}

impl Drop for SegmentGuard {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            handle.mark(self.name, self.start.elapsed());
        }
    }
}

/// Start timing a named segment against the request's telemetry record.
/// Used by generated code (the `seed` step of prefetch-backed pages); apps
/// should prefer the [`Timing`] extractor.
pub fn start_segment(extensions: &http::Extensions, name: &'static str) -> SegmentGuard {
    SegmentGuard {
        handle: extensions.get::<Handle>().cloned(),
        name,
        start: Instant::now(),
    }
}

/// Extractor for adding custom segments to a route's timing breakdown:
///
/// ```ignore
/// pub async fn get(timing: nextrs::Timing) -> Json<Vec<Todo>> {
///     let todos = timing.span("db", fetch_todos()).await;
///     Json(todos)
/// }
/// ```
///
/// Segments appear in the `Server-Timing` header and the tracing summary
/// event. Outside a nextrs request (unit tests, direct calls) every method is
/// a no-op, so handlers stay testable in isolation.
#[derive(Clone, Default)]
pub struct Timing(Option<Handle>);

impl Timing {
    /// A `Timing` that records nothing. What the extractor yields outside a
    /// nextrs request.
    pub fn noop() -> Self {
        Self(None)
    }

    /// Build from request extensions. Used by the seed companions
    /// `#[nextrs::api]` generates, which hold `&Extensions` rather than a
    /// request; apps should extract [`Timing`] directly.
    pub fn from_extensions(extensions: &http::Extensions) -> Self {
        Self(extensions.get::<Handle>().cloned())
    }

    /// Await `fut`, recording its duration as segment `name`.
    pub async fn span<T, F>(&self, name: &'static str, fut: F) -> T
    where
        F: Future<Output = T>,
    {
        let start = Instant::now();
        let out = fut.await;
        let dur = start.elapsed();
        if let Some(handle) = &self.0 {
            handle.mark(name, dur);
            tracing::trace!(
                target: "nextrs::telemetry",
                segment = name,
                dur_ms = ms(dur),
                "segment",
            );
        }
        out
    }

    /// Record an already-measured duration as segment `name`.
    pub fn mark(&self, name: impl Into<Cow<'static, str>>, dur: Duration) {
        if let Some(handle) = &self.0 {
            handle.mark(name, dur);
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Timing {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Infallible> {
        Ok(Self(parts.extensions.get::<Handle>().cloned()))
    }
}

/// Router-wide layer: reads the telemetry handle the route handler left in
/// response extensions, stamps the `Server-Timing` header, and emits the
/// summary event. Installed outermost (after `health::stamp`) so the cold
/// flag it reads is already on the response. Responses without a handle
/// (static files, health endpoint) pass through untouched.
pub(crate) async fn record(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let mut resp = next.run(req).await;
    let Some(handle) = resp.extensions().get::<Handle>().cloned() else {
        return resp;
    };
    handle.set_status(resp.status().as_u16());
    if let Some(cold) = resp.headers().get("x-nextrs-cold") {
        handle.set_cold(cold == "1");
    }
    if server_timing_enabled() {
        if let Ok(value) = http::HeaderValue::from_str(&server_timing_value(&handle)) {
            resp.headers_mut().insert("server-timing", value);
        }
    }
    if !handle.is_streaming() {
        handle.emit();
    }
    resp
}

/// `Server-Timing` is on unless `NEXTRS_SERVER_TIMING` opts out (`0`, `false`,
/// `off`). Checked once per process.
fn server_timing_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        server_timing_opt_out(std::env::var("NEXTRS_SERVER_TIMING").ok().as_deref())
    })
}

fn server_timing_opt_out(var: Option<&str>) -> bool {
    !matches!(var, Some("0") | Some("false") | Some("off"))
}

/// Build the `Server-Timing` header value. Streaming responses report only
/// what is known at header time (middleware + a `streaming` marker); the full
/// breakdown reaches tracing when the stream finishes.
fn server_timing_value(handle: &RouteTelemetry) -> String {
    let mut parts = Vec::new();
    if let Some(mw) = handle.mw_ms() {
        parts.push(format!("mw;dur={mw:.1}"));
    }
    if handle.is_streaming() {
        parts.push("streaming".to_string());
    } else {
        for (name, ms) in handle.segments_ms() {
            parts.push(format!("{};dur={ms:.1}", metric_name(&name)));
        }
        if let Some(h) = handle.handler_ms() {
            parts.push(format!("handler;dur={h:.1}"));
        }
        parts.push(format!("total;dur={:.1}", ms(handle.start.elapsed())));
    }
    parts.push(format!(
        "route;desc=\"{}\"",
        handle.route().replace('"', "")
    ));
    parts.join(", ")
}

/// Server-Timing metric names are RFC 8941 tokens. Segment names come from
/// app code; squash anything that would break the header.
fn metric_name(name: &str) -> Cow<'_, str> {
    if name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '*' | '/'))
        && !name.is_empty()
    {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(
            name.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '*' | '/') {
                        c
                    } else {
                        '-'
                    }
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_record_in_order() {
        let t = RouteTelemetry::new("/x", http::Method::GET);
        t.mark("seed", Duration::from_millis(430));
        t.mark("db", Duration::from_millis(12));
        let segs = t.segments_ms();
        assert_eq!(segs[0].0, "seed");
        assert!((segs[0].1 - 430.0).abs() < 1.0);
        assert_eq!(segs[1].0, "db");
    }

    #[test]
    fn server_timing_value_full_breakdown() {
        let t = RouteTelemetry::new("/todos/{id}", http::Method::GET);
        t.record_mw(Duration::from_micros(1200));
        t.mark("seed", Duration::from_millis(430));
        t.record_handler(Duration::from_millis(445));
        let v = server_timing_value(&t);
        assert!(v.starts_with("mw;dur=1.2"), "{v}");
        assert!(v.contains("seed;dur=430.0"), "{v}");
        assert!(v.contains("handler;dur=445.0"), "{v}");
        assert!(v.contains("total;dur="), "{v}");
        assert!(v.ends_with("route;desc=\"/todos/{id}\""), "{v}");
    }

    #[test]
    fn server_timing_value_streaming_is_partial() {
        let t = RouteTelemetry::new("/slow", http::Method::GET);
        t.record_mw(Duration::from_micros(800));
        t.set_streaming();
        // Recorded later by the stream — must NOT appear in the header.
        t.record_handler(Duration::from_millis(500));
        let v = server_timing_value(&t);
        assert!(v.contains("streaming"), "{v}");
        assert!(!v.contains("handler"), "{v}");
        assert!(!v.contains("total"), "{v}");
        assert!(v.contains("route;desc=\"/slow\""), "{v}");
    }

    #[test]
    fn metric_names_are_sanitized() {
        assert_eq!(metric_name("db"), "db");
        assert_eq!(metric_name("db query, extra"), "db-query--extra");
        assert_eq!(metric_name(""), "");
    }

    #[test]
    fn timing_noop_is_inert() {
        let timing = Timing::noop();
        timing.mark("db", Duration::from_millis(5));
        // span still runs the future and returns its output
        let out = futures_lite_block_on(timing.span("db", async { 7 }));
        assert_eq!(out, 7);
    }

    // Minimal executor so the no-op test doesn't need a runtime dep here.
    fn futures_lite_block_on<T>(fut: impl Future<Output = T>) -> T {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        fn raw() -> RawWaker {
            fn no_op(_: *const ()) {}
            fn clone(_: *const ()) -> RawWaker {
                raw()
            }
            RawWaker::new(
                std::ptr::null(),
                &RawWakerVTable::new(clone, no_op, no_op, no_op),
            )
        }
        let waker = unsafe { Waker::from_raw(raw()) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(fut);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    #[test]
    fn emit_is_idempotent() {
        let t = RouteTelemetry::new("/x", http::Method::GET);
        t.emit();
        t.emit(); // second call (and Drop) must not double-fire — no panic is
        // the observable here; the once-guard is the swap in emit().
        assert!(t.emitted.load(Ordering::Relaxed));
    }
}
