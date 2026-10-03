//! Request and job log capture — keep your logs for days, not an hour.
//!
//! Vercel's Hobby plan keeps runtime logs for one hour. This module captures
//! the `tracing` lines your code emits *inside* a request (or a job attempt)
//! and saves them, with the route's telemetry, as one record per request in a
//! store you own (Turso in production, memory in dev). The admin dashboard
//! (`/__nx/admin/logs`) and `GET /__nx/admin/api/logs` read it back.
//!
//! How lines find their request: the router runs every route inside a
//! task-local [`Capture`]; the [`layer`] appends each event to whichever
//! capture is current. [`WaitUntil`](crate::WaitUntil) carries the capture
//! into background futures (marking their lines `after_response`) and counts
//! them, so the record is saved once, after the request's background work
//! finishes — complete, not two halves.
//!
//! Blind spots, by construction: anything that kills the process before the
//! record flushes (init panics, timeouts, OOM) and platform/edge errors. Those
//! remain only in the platform's own log view.
//!
//! Wiring (one line in each process entry, next to the fmt layer):
//!
//! ```ignore
//! use tracing_subscriber::prelude::*;
//! tracing_subscriber::registry()
//!     .with(tracing_subscriber::EnvFilter::new("info"))
//!     .with(tracing_subscriber::fmt::layer())
//!     .with(nextrs::logs::layer())
//!     .init();
//! ```

// Without the `logs` feature the capture plumbing is compiled (WaitUntil and
// jobs call into it) but nothing feeds or saves it.
#![cfg_attr(not(feature = "logs"), allow(dead_code))]

use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Most lines kept per capture; later lines are counted, not stored, so a
/// chatty loop can't balloon one record.
pub const MAX_LINES: usize = 200;

/// One captured `tracing` event.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LogLine {
    /// Milliseconds since the capture started (request or attempt start).
    pub t_ms: f64,
    pub level: String,
    pub target: String,
    pub msg: String,
    /// Structured fields other than the message.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub fields: serde_json::Map<String, serde_json::Value>,
    /// Emitted by `WaitUntil` work after the response went out.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub after_response: bool,
}

/// A per-request (or per-job-attempt) line buffer plus a count of the
/// background futures still writing to it.
pub struct Capture {
    start: Instant,
    lines: Mutex<Vec<LogLine>>,
    dropped: AtomicUsize,
    pending: AtomicUsize,
    idle: tokio::sync::Notify,
    responded: AtomicBool,
    /// The request's record as of its telemetry summary (set by `emit`, which
    /// also runs from the telemetry's `Drop` for abandoned streams), so the
    /// saver never needs to keep the request's telemetry alive.
    snapshot: Mutex<Option<RequestLog>>,
}

impl Capture {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            start: Instant::now(),
            lines: Mutex::new(Vec::new()),
            dropped: AtomicUsize::new(0),
            pending: AtomicUsize::new(0),
            idle: tokio::sync::Notify::new(),
            responded: AtomicBool::new(false),
            snapshot: Mutex::new(None),
        })
    }

    pub(crate) fn set_snapshot(&self, record: RequestLog) {
        if let Ok(mut slot) = self.snapshot.lock() {
            *slot = Some(record);
        }
    }

    pub(crate) fn has_snapshot(&self) -> bool {
        self.snapshot.lock().map(|s| s.is_some()).unwrap_or(false)
    }

    /// The snapshot with this capture's current lines (background work may
    /// have logged since the summary was taken).
    pub(crate) fn take_record(&self) -> Option<RequestLog> {
        let mut record = self.snapshot.lock().ok()?.take()?;
        record.lines = self.lines();
        record.level = self.max_level();
        record.dropped_lines = self.dropped();
        Some(record)
    }

    fn push(&self, mut line: LogLine) {
        line.t_ms = self.start.elapsed().as_secs_f64() * 1000.0;
        line.after_response = self.responded.load(Ordering::Relaxed);
        let Ok(mut lines) = self.lines.lock() else { return };
        if lines.len() < MAX_LINES {
            lines.push(line);
        } else {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Lines captured so far, in order.
    pub fn lines(&self) -> Vec<LogLine> {
        self.lines.lock().map(|l| l.clone()).unwrap_or_default()
    }

    /// Lines past [`MAX_LINES`] that were counted but not kept.
    pub fn dropped(&self) -> usize {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Highest level seen (`"error"` > `"warn"` > `"info"` > …), if any line.
    pub fn max_level(&self) -> Option<String> {
        self.lines()
            .iter()
            .map(|l| level_rank(&l.level))
            .max()
            .map(|r| LEVELS[r].to_string())
    }

    /// The response has gone out: later lines are `after_response`.
    pub(crate) fn mark_responded(&self) {
        self.responded.store(true, Ordering::Relaxed);
    }

    pub(crate) fn begin_background(&self) {
        self.pending.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) fn end_background(&self) {
        if self.pending.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.idle.notify_waiters();
        }
    }

    /// Wait until no background future is still writing, or `max` elapses.
    pub(crate) async fn wait_idle(&self, max: Duration) {
        let deadline = tokio::time::Instant::now() + max;
        loop {
            let notified = self.idle.notified();
            if self.pending.load(Ordering::Acquire) == 0 {
                return;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return;
            }
        }
    }
}

const LEVELS: [&str; 5] = ["trace", "debug", "info", "warn", "error"];

fn level_rank(level: &str) -> usize {
    LEVELS.iter().position(|l| *l == level).unwrap_or(2)
}

tokio::task_local! {
    static CAPTURE: Arc<Capture>;
}

/// The capture the current task is writing to, if any.
pub fn current() -> Option<Arc<Capture>> {
    CAPTURE.try_with(Arc::clone).ok()
}

/// Run `fut` with `capture` as the current capture.
pub fn scope<F: Future>(capture: Arc<Capture>, fut: F) -> impl Future<Output = F::Output> {
    CAPTURE.scope(capture, fut)
}

/// Wrap a background future so its lines land in the current capture (if
/// any) and the capture knows to wait for it. Used by `WaitUntil`.
pub(crate) fn wrap_background<F>(fut: F) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>>
where
    F: Future<Output = ()> + Send + 'static,
{
    struct Done(Arc<Capture>);
    impl Drop for Done {
        fn drop(&mut self) {
            self.0.end_background();
        }
    }
    match current() {
        Some(cap) => {
            cap.begin_background();
            // Built now, not inside the async block: a future dropped before
            // its first poll (runtime shutdown, a discarding scheduler) must
            // still release its count, or the record waits out MAX_WAIT.
            let done = Done(Arc::clone(&cap));
            Box::pin(CAPTURE.scope(cap, async move {
                let _done = done;
                fut.await;
            }))
        }
        None => Box::pin(fut),
    }
}

// ------------------------------------------------------------------ the layer

#[cfg(feature = "logs")]
pub use layer_impl::{CaptureLayer, layer};

#[cfg(feature = "logs")]
mod layer_impl {
    use super::*;
    use tracing::field::{Field, Visit};

    /// The `tracing_subscriber` layer that feeds captures. Install it once in
    /// each process entry (see the module docs).
    pub fn layer() -> CaptureLayer {
        CaptureLayer { _priv: () }
    }

    /// See [`layer`].
    pub struct CaptureLayer {
        _priv: (),
    }

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for CaptureLayer {
        fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
            let meta = event.metadata();
            // The per-request summary is the record itself, not a line in it.
            if meta.target() == "nextrs::telemetry" {
                return;
            }
            let Some(cap) = current() else { return };
            let mut visitor = FieldVisitor::default();
            event.record(&mut visitor);
            cap.push(LogLine {
                t_ms: 0.0,
                level: meta.level().as_str().to_ascii_lowercase(),
                target: meta.target().to_string(),
                msg: visitor.message,
                fields: visitor.fields,
                after_response: false,
            });
        }
    }

    #[derive(Default)]
    struct FieldVisitor {
        message: String,
        fields: serde_json::Map<String, serde_json::Value>,
    }

    impl FieldVisitor {
        fn put(&mut self, field: &Field, value: serde_json::Value) {
            if field.name() == "message" {
                self.message = match value {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                };
            } else {
                self.fields.insert(field.name().to_string(), value);
            }
        }
    }

    impl Visit for FieldVisitor {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.put(field, serde_json::Value::String(format!("{value:?}")));
        }
        fn record_str(&mut self, field: &Field, value: &str) {
            self.put(field, serde_json::Value::String(value.to_string()));
        }
        fn record_i64(&mut self, field: &Field, value: i64) {
            self.put(field, value.into());
        }
        fn record_u64(&mut self, field: &Field, value: u64) {
            self.put(field, value.into());
        }
        fn record_f64(&mut self, field: &Field, value: f64) {
            self.put(field, value.into());
        }
        fn record_bool(&mut self, field: &Field, value: bool) {
            self.put(field, value.into());
        }
        fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
            self.put(field, serde_json::Value::String(value.to_string()));
        }
    }
}

// -------------------------------------------------------------- request records

/// One saved request: the route telemetry plus the lines it logged.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestLog {
    pub id: String,
    /// Unix ms when the request started.
    pub ts: i64,
    pub method: String,
    pub route: String,
    pub status: u16,
    pub ms: f64,
    pub cold: bool,
    /// Highest line level, for filtering (`None` when nothing was logged).
    pub level: Option<String>,
    /// `(name, ms)` timing segments: `mw`, `handler`, and `Timing` spans.
    pub segments: Vec<(String, f64)>,
    pub lines: Vec<LogLine>,
    #[serde(default)]
    pub dropped_lines: usize,
}

/// Filter for [`LogStore::query`]. Empty filter = newest records.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LogQuery {
    pub route: Option<String>,
    /// Minimum status, e.g. `500` for "5xx".
    pub status_min: Option<u16>,
    /// Minimum level: records whose highest line is at least this.
    pub level: Option<String>,
    /// Only records at or after this unix ms.
    pub since: Option<i64>,
    /// Page backwards: only records strictly before this unix ms.
    pub before: Option<i64>,
    pub limit: Option<u32>,
}

impl LogQuery {
    fn matches(&self, r: &RequestLog) -> bool {
        self.route.as_ref().is_none_or(|route| &r.route == route)
            && self.status_min.is_none_or(|min| r.status >= min)
            && self.level.as_ref().is_none_or(|min| {
                r.level.as_ref().is_some_and(|l| level_rank(l) >= level_rank(min))
            })
            && self.since.is_none_or(|since| r.ts >= since)
            && self.before.is_none_or(|before| r.ts < before)
    }
    fn limit(&self) -> usize {
        self.limit.unwrap_or(100).min(500) as usize
    }
}

/// A storage-backend failure (stringly-typed, like the jobs store's).
#[derive(Clone, Debug)]
pub struct LogStoreError(pub String);

impl std::fmt::Display for LogStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for LogStoreError {}

type StoreFuture<'a, T> =
    std::pin::Pin<Box<dyn Future<Output = Result<T, LogStoreError>> + Send + 'a>>;

/// Where request records live. The framework ships [`MemoryLogStore`] (dev)
/// and, with the `libsql` feature, a Turso store.
pub trait LogStore: Send + Sync {
    fn insert(&self, record: RequestLog) -> StoreFuture<'_, ()>;
    /// Newest first.
    fn query(&self, query: LogQuery) -> StoreFuture<'_, Vec<RequestLog>>;
    fn get(&self, id: &str) -> StoreFuture<'_, Option<RequestLog>>;
    /// Delete records older than `cutoff` (unix ms). Returns how many.
    fn prune(&self, cutoff: i64) -> StoreFuture<'_, u64>;
}

/// In-memory [`LogStore`]: the newest 2,000 records of this process. The dev
/// default; on Vercel each instance would hold its own, so production uses
/// the libsql store.
pub struct MemoryLogStore {
    records: Mutex<VecDeque<RequestLog>>,
    cap: usize,
}

impl MemoryLogStore {
    pub fn new() -> Self {
        Self::with_capacity(2000)
    }
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            records: Mutex::new(VecDeque::new()),
            cap,
        }
    }
}

impl Default for MemoryLogStore {
    fn default() -> Self {
        Self::new()
    }
}

impl LogStore for MemoryLogStore {
    fn insert(&self, record: RequestLog) -> StoreFuture<'_, ()> {
        Box::pin(async move {
            let mut records = self
                .records
                .lock()
                .map_err(|e| LogStoreError(e.to_string()))?;
            records.push_front(record);
            records.truncate(self.cap);
            Ok(())
        })
    }
    fn query(&self, query: LogQuery) -> StoreFuture<'_, Vec<RequestLog>> {
        Box::pin(async move {
            let records = self
                .records
                .lock()
                .map_err(|e| LogStoreError(e.to_string()))?;
            let mut out: Vec<RequestLog> =
                records.iter().filter(|r| query.matches(r)).cloned().collect();
            out.sort_by_key(|r| std::cmp::Reverse(r.ts));
            out.truncate(query.limit());
            Ok(out)
        })
    }
    fn get(&self, id: &str) -> StoreFuture<'_, Option<RequestLog>> {
        let id = id.to_string();
        Box::pin(async move {
            let records = self
                .records
                .lock()
                .map_err(|e| LogStoreError(e.to_string()))?;
            Ok(records.iter().find(|r| r.id == id).cloned())
        })
    }
    fn prune(&self, cutoff: i64) -> StoreFuture<'_, u64> {
        Box::pin(async move {
            let mut records = self
                .records
                .lock()
                .map_err(|e| LogStoreError(e.to_string()))?;
            let before = records.len();
            records.retain(|r| r.ts >= cutoff);
            Ok((before - records.len()) as u64)
        })
    }
}

static STORE: std::sync::OnceLock<Arc<dyn LogStore>> = std::sync::OnceLock::new();

/// Install a custom [`LogStore`]. First call wins; call before the first
/// request. Returns `Err` when a store was already resolved.
pub fn set_store(store: Arc<dyn LogStore>) -> Result<(), Arc<dyn LogStore>> {
    STORE.set(store)
}

/// Resolve the process-wide log store: an explicit [`set_store`]; else, with
/// the `libsql` feature, `NEXTRS_LOGS_DB_URL` / `NEXTRS_DB_URL` /
/// `TURSO_DATABASE_URL`; else memory.
pub fn store() -> &'static Arc<dyn LogStore> {
    #[cfg(feature = "libsql")]
    {
        if STORE.get().is_none() {
            if let Some((url, token)) = crate::db::env_url(&["NEXTRS_LOGS_DB_URL"], &["NEXTRS_LOGS_DB_TOKEN"]) {
                let _ = STORE.set(Arc::new(libsql_store::LibsqlLogStore::new(url, token)));
            }
        }
    }
    STORE.get_or_init(|| Arc::new(MemoryLogStore::new()))
}

/// Is request logging on? Default on; `NEXTRS_LOGS=0` turns it off.
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("NEXTRS_LOGS").ok().as_deref(),
            Some("0") | Some("false") | Some("off")
        )
    })
}

/// Sample rate for fast, successful, quiet requests (`NEXTRS_LOGS_SAMPLE`,
/// 0.0–1.0, default 1.0 = keep everything). Errors, warnings, slow requests,
/// and cold starts are always kept.
fn sample_rate() -> f64 {
    static RATE: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *RATE.get_or_init(|| {
        std::env::var("NEXTRS_LOGS_SAMPLE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| v.clamp(0.0, 1.0))
            .unwrap_or(1.0)
    })
}

/// Days to keep request records (`NEXTRS_LOGS_RETENTION_DAYS`, default 14).
pub fn retention_days() -> i64 {
    std::env::var("NEXTRS_LOGS_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(14)
}

/// Delete records past [`retention_days`]. The jobs sweep calls this, so a
/// cron hitting `/__nx/jobs/sweep` also enforces log retention.
pub async fn prune_expired() -> Result<u64, LogStoreError> {
    store().prune(now_ms() - retention_days() * 86_400_000).await
}

/// Requests slower than this are always kept.
const SLOW_MS: f64 = 500.0;

/// Tail sampling: decided after the request, when the outcome is known.
pub(crate) fn keep(record: &RequestLog, roll: f64) -> bool {
    record.status >= 500
        || record
            .level
            .as_deref()
            .is_some_and(|l| level_rank(l) >= level_rank("warn"))
        || record.ms >= SLOW_MS
        || record.cold
        || roll < sample_rate()
}

/// Unix ms now.
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Pseudo-random id / sampling roll without an RNG dependency.
pub(crate) fn random_u64() -> u64 {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut z = nanos ^ ((COUNTER.fetch_add(1, Ordering::Relaxed) as u64) << 32)
        ^ ((std::process::id() as u64) << 16);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

/// Save a finished request's record (if sampling keeps it). Errors are
/// reported to stderr-backed tracing *outside* any capture, never retried.
pub(crate) async fn save(record: RequestLog) {
    let roll = (random_u64() % 10_000) as f64 / 10_000.0;
    if !keep(&record, roll) {
        return;
    }
    if let Err(e) = store().insert(record).await {
        tracing::warn!(target: "nextrs::telemetry", error = %e, "request log insert failed");
    }
}

#[cfg(feature = "libsql")]
mod libsql_store;
#[cfg(feature = "libsql")]
pub use libsql_store::LibsqlLogStore;

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, ts: i64, route: &str, status: u16, level: Option<&str>) -> RequestLog {
        RequestLog {
            id: id.into(),
            ts,
            method: "GET".into(),
            route: route.into(),
            status,
            ms: 3.0,
            cold: false,
            level: level.map(Into::into),
            segments: vec![],
            lines: vec![],
            dropped_lines: 0,
        }
    }

    #[tokio::test]
    async fn memory_store_filters_newest_first() {
        let s = MemoryLogStore::new();
        s.insert(record("a", 1, "/api/todos", 200, Some("info"))).await.unwrap();
        s.insert(record("b", 2, "/api/todos", 500, Some("error"))).await.unwrap();
        s.insert(record("c", 3, "/", 200, None)).await.unwrap();

        let all = s.query(LogQuery::default()).await.unwrap();
        assert_eq!(all.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["c", "b", "a"]);

        let errors = s
            .query(LogQuery { status_min: Some(500), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].id, "b");

        let warnish = s
            .query(LogQuery { level: Some("warn".into()), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(warnish.len(), 1);

        let todos_before_2 = s
            .query(LogQuery { route: Some("/api/todos".into()), before: Some(2), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(todos_before_2.len(), 1);
        assert_eq!(todos_before_2[0].id, "a");

        assert_eq!(s.prune(2).await.unwrap(), 1);
        assert!(s.get("a").await.unwrap().is_none());
        assert!(s.get("b").await.unwrap().is_some());
    }

    #[test]
    fn tail_sampling_always_keeps_problems() {
        let ok = record("a", 1, "/", 200, Some("info"));
        let err = record("b", 1, "/", 500, None);
        let warned = record("c", 1, "/", 200, Some("warn"));
        // A roll of 1.0 is never below the rate, so only "always keep" rules pass.
        assert!(keep(&err, 1.0));
        assert!(keep(&warned, 1.0));
        assert!(!keep(&ok, 1.0));
        assert!(keep(&ok, 0.5)); // default rate 1.0 keeps everything else
    }

    #[tokio::test]
    async fn a_background_future_dropped_unpolled_releases_its_count() {
        let cap = Capture::new();
        let fut = scope(Arc::clone(&cap), async { wrap_background(async {}) }).await;
        drop(fut); // never polled
        let started = std::time::Instant::now();
        cap.wait_idle(Duration::from_secs(5)).await;
        assert!(started.elapsed() < Duration::from_secs(1), "wait_idle stalled on a leaked count");
    }

    #[tokio::test]
    async fn background_work_is_counted_and_awaited() {
        let cap = Capture::new();
        let flag = Arc::new(AtomicBool::new(false));
        let f2 = Arc::clone(&flag);
        let bg = scope(Arc::clone(&cap), async move {
            wrap_background(async move {
                tokio::time::sleep(Duration::from_millis(30)).await;
                f2.store(true, Ordering::SeqCst);
            })
        })
        .await;
        tokio::spawn(bg);
        cap.wait_idle(Duration::from_secs(2)).await;
        assert!(flag.load(Ordering::SeqCst), "wait_idle returned before the background future finished");
    }
}
