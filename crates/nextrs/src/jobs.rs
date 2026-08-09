//! First-party background jobs, executed through `WaitUntil`.
//!
//! A job is a `pub async fn` in `app/jobs/<name>/job.rs` annotated with
//! `#[nextrs::job]`. Calling that function from app code does not run the
//! body — the macro re-emits the name as a typed *enqueue* wrapper that
//! persists a job row and POSTs the job's own reserved route
//! (`/__nx/jobs/<name>`) on this deployment. That route claims the row,
//! answers `202` immediately, and runs the real body behind the request's
//! [`WaitUntil`](crate::WaitUntil) — platform-backed on Vercel, `tokio::spawn`
//! locally — with a per-job timeout. `Err`/timeout mark the row failed with
//! exponential back-off (`30s · 2^(n-1)`, capped at 1h) until `max_attempts`,
//! then `dead`. The authed sweep route (`/__nx/jobs/sweep`) re-delivers due
//! rows; drive it from any cron.
//!
//! Durability lives in the row, not the kick-off POST: enqueue still returns
//! `Ok` when delivery fails (the sweep picks the row up), and double delivery
//! is harmless because [`JobStore::claim`] is atomic.
//!
//! Storage resolves lazily, once per process: an explicit [`set_store`] wins;
//! else `NEXTRS_JOBS_DB_URL`/`NEXTRS_JOBS_DB_TOKEN` (falling back to
//! `TURSO_DATABASE_URL`/`TURSO_AUTH_TOKEN`) selects the libsql store (feature
//! `jobs-libsql`); else the in-memory store. On Vercel a missing DB or
//! missing `NEXTRS_JOBS_SECRET` fails loud at enqueue — never a silently
//! non-durable queue in production.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Reserved path prefix for the per-job run routes and the sweep/status
/// endpoints.
pub const NX_JOBS_PREFIX: &str = "/__nx/jobs";

/// Header carrying the shared jobs secret on run/sweep/status requests.
pub const JOBS_SECRET_HEADER: &str = "x-nextrs-jobs-secret";

// ---------------------------------------------------------------- identifiers

/// Opaque job identifier — 32 hex chars, generated at enqueue time so the
/// caller's [`JobHandle`] and the stored row agree by construction.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(pub String);

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Random-enough without an RNG dependency: two SplitMix64 finalizer rounds
/// over (wall-clock nanos, a process counter, the pid) — the same trick as
/// `health::boot_id`, widened to 128 bits. Uniqueness matters here (it's a
/// primary key); unguessability is not load-bearing (the routes are authed).
fn generate_job_id() -> JobId {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    fn splitmix(mut z: u64) -> u64 {
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id() as u64;
    let a = splitmix(nanos ^ (pid << 32) ^ count);
    let b = splitmix(a ^ count.rotate_left(17) ^ pid);
    JobId(format!("{a:016x}{b:016x}"))
}

/// Unix milliseconds now — the row timestamps' unit.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ----------------------------------------------------------------------- rows

/// Lifecycle of a job row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Dead,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            JobStatus::Queued => "queued",
            JobStatus::Running => "running",
            JobStatus::Succeeded => "succeeded",
            JobStatus::Failed => "failed",
            JobStatus::Dead => "dead",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "queued" => JobStatus::Queued,
            "running" => JobStatus::Running,
            "succeeded" => JobStatus::Succeeded,
            "failed" => JobStatus::Failed,
            "dead" => JobStatus::Dead,
            _ => return None,
        })
    }
}

/// A stored job.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobRow {
    pub id: JobId,
    pub name: String,
    pub payload: serde_json::Value,
    pub status: JobStatus,
    pub attempts: u32,
    pub max_attempts: u32,
    /// Unix ms when the job is (next) due; `None` on terminal rows.
    pub next_run_at: Option<i64>,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

// --------------------------------------------------------------------- errors

/// A storage backend failure, stringly-typed on purpose: backends differ, and
/// every caller either logs it or maps it to a 500.
#[derive(Clone, Debug)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for StoreError {}

/// Why an enqueue call failed. Delivery failure is *not* here — a persisted
/// row whose kick-off POST failed is still `Ok` (`delivered: false`).
#[derive(Debug)]
pub enum EnqueueError {
    /// The payload didn't serialize.
    Payload(serde_json::Error),
    /// The job row could not be persisted.
    Store(StoreError),
    /// Running on Vercel without the required configuration
    /// (`NEXTRS_JOBS_SECRET`, and a database for `jobs-libsql` builds).
    Config(String),
}

impl std::fmt::Display for EnqueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnqueueError::Payload(e) => write!(f, "job payload did not serialize: {e}"),
            EnqueueError::Store(e) => write!(f, "job row insert failed: {e}"),
            EnqueueError::Config(msg) => f.write_str(msg),
        }
    }
}
impl std::error::Error for EnqueueError {}
impl From<serde_json::Error> for EnqueueError {
    fn from(e: serde_json::Error) -> Self {
        EnqueueError::Payload(e)
    }
}
impl From<StoreError> for EnqueueError {
    fn from(e: StoreError) -> Self {
        EnqueueError::Store(e)
    }
}

// -------------------------------------------------------------------- handle

/// What an enqueue call returns: a handle if the caller wants one, ignorable
/// otherwise.
#[derive(Clone, Debug)]
pub struct JobHandle {
    pub id: JobId,
    /// Whether the kick-off POST reached the job route. `false` is not a
    /// failure — the row is durable and the sweep will deliver it.
    pub delivered: bool,
}

impl JobHandle {
    /// Read the job's current row (status, attempts, last error).
    pub async fn status(&self) -> Result<Option<JobRow>, StoreError> {
        store()?.get(&self.id).await
    }
}

// --------------------------------------------------------------------- store

type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StoreError>> + Send + 'a>>;

/// Storage backend for job rows. Implement to bring your own database; the
/// framework ships [`MemoryJobStore`] (local dev) and, behind the
/// `jobs-libsql` feature, a Turso/libsql store.
pub trait JobStore: Send + Sync {
    fn insert(&self, row: JobRow) -> StoreFuture<'_, ()>;
    /// Atomically move a `queued`/`failed` row to `running` and bump
    /// `attempts`. `None` when the row is missing or not claimable — the
    /// double-delivery guard.
    fn claim(&self, id: &JobId) -> StoreFuture<'_, Option<JobRow>>;
    fn mark_succeeded(&self, id: &JobId) -> StoreFuture<'_, ()>;
    /// `next_run_at: Some(ms)` → retryable `failed`; `None` → terminal `dead`.
    fn mark_failed(&self, id: &JobId, error: &str, next_run_at: Option<i64>) -> StoreFuture<'_, ()>;
    /// Non-terminal rows due at or before `now`, oldest first.
    fn due(&self, now: i64, limit: u32) -> StoreFuture<'_, Vec<JobRow>>;
    /// `running` rows untouched since `cutoff` go back to due-now `failed`
    /// (instance died mid-run). Returns how many were reclaimed.
    fn reclaim_stale(&self, cutoff: i64) -> StoreFuture<'_, u32>;
    fn get(&self, id: &JobId) -> StoreFuture<'_, Option<JobRow>>;
    /// Recent rows, newest first — the status endpoint's data.
    fn recent(&self, limit: u32) -> StoreFuture<'_, Vec<JobRow>>;
}

/// In-memory [`JobStore`]. The local-dev default: durable within the process,
/// which is exactly the lifetime local work has. Never silently used on
/// Vercel — see [`enqueue`].
#[derive(Default)]
pub struct MemoryJobStore {
    rows: Mutex<HashMap<String, JobRow>>,
}

impl MemoryJobStore {
    pub fn new() -> Self {
        Self::default()
    }
    fn with<T>(&self, f: impl FnOnce(&mut HashMap<String, JobRow>) -> T) -> Result<T, StoreError> {
        let mut rows = self.rows.lock().map_err(|e| StoreError(e.to_string()))?;
        Ok(f(&mut rows))
    }
}

impl JobStore for MemoryJobStore {
    fn insert(&self, row: JobRow) -> StoreFuture<'_, ()> {
        Box::pin(async move {
            self.with(|rows| {
                rows.insert(row.id.0.clone(), row);
            })
        })
    }
    fn claim(&self, id: &JobId) -> StoreFuture<'_, Option<JobRow>> {
        let id = id.clone();
        Box::pin(async move {
            self.with(|rows| {
                let row = rows.get_mut(&id.0)?;
                if !matches!(row.status, JobStatus::Queued | JobStatus::Failed) {
                    return None;
                }
                row.status = JobStatus::Running;
                row.attempts += 1;
                row.next_run_at = None;
                row.updated_at = now_ms();
                Some(row.clone())
            })
        })
    }
    fn mark_succeeded(&self, id: &JobId) -> StoreFuture<'_, ()> {
        let id = id.clone();
        Box::pin(async move {
            self.with(|rows| {
                if let Some(row) = rows.get_mut(&id.0) {
                    row.status = JobStatus::Succeeded;
                    row.next_run_at = None;
                    row.updated_at = now_ms();
                }
            })
        })
    }
    fn mark_failed(&self, id: &JobId, error: &str, next_run_at: Option<i64>) -> StoreFuture<'_, ()> {
        let id = id.clone();
        let error = error.to_string();
        Box::pin(async move {
            self.with(|rows| {
                if let Some(row) = rows.get_mut(&id.0) {
                    row.status = if next_run_at.is_some() {
                        JobStatus::Failed
                    } else {
                        JobStatus::Dead
                    };
                    row.next_run_at = next_run_at;
                    row.last_error = Some(error);
                    row.updated_at = now_ms();
                }
            })
        })
    }
    fn due(&self, now: i64, limit: u32) -> StoreFuture<'_, Vec<JobRow>> {
        Box::pin(async move {
            self.with(|rows| {
                let mut due: Vec<JobRow> = rows
                    .values()
                    .filter(|r| {
                        matches!(r.status, JobStatus::Queued | JobStatus::Failed)
                            && r.next_run_at.is_some_and(|t| t <= now)
                    })
                    .cloned()
                    .collect();
                due.sort_by_key(|r| r.next_run_at);
                due.truncate(limit as usize);
                due
            })
        })
    }
    fn reclaim_stale(&self, cutoff: i64) -> StoreFuture<'_, u32> {
        Box::pin(async move {
            self.with(|rows| {
                let mut n = 0;
                for row in rows.values_mut() {
                    if row.status == JobStatus::Running && row.updated_at < cutoff {
                        row.status = JobStatus::Failed;
                        row.next_run_at = Some(now_ms());
                        row.last_error
                            .get_or_insert_with(|| "reclaimed: instance died mid-run".into());
                        row.updated_at = now_ms();
                        n += 1;
                    }
                }
                n
            })
        })
    }
    fn get(&self, id: &JobId) -> StoreFuture<'_, Option<JobRow>> {
        let id = id.clone();
        Box::pin(async move { self.with(|rows| rows.get(&id.0).cloned()) })
    }
    fn recent(&self, limit: u32) -> StoreFuture<'_, Vec<JobRow>> {
        Box::pin(async move {
            self.with(|rows| {
                let mut all: Vec<JobRow> = rows.values().cloned().collect();
                all.sort_by_key(|r| std::cmp::Reverse(r.created_at));
                all.truncate(limit as usize);
                all
            })
        })
    }
}

// ------------------------------------------------------------ global wiring

static STORE: OnceLock<Arc<dyn JobStore>> = OnceLock::new();

/// Install a custom [`JobStore`] (tests, alternative backends). First call
/// wins — call it before the first enqueue/run. Returns `Err` when a store
/// was already resolved.
pub fn set_store(store: Arc<dyn JobStore>) -> Result<(), Arc<dyn JobStore>> {
    STORE.set(store)
}

fn on_vercel() -> bool {
    std::env::var_os("VERCEL").is_some()
}

/// Resolve the process-wide store. See the module docs for the order.
pub(crate) fn store() -> Result<&'static Arc<dyn JobStore>, StoreError> {
    #[cfg(feature = "jobs-libsql")]
    {
        if STORE.get().is_none() {
            if let Some(store) = libsql_store::from_env() {
                let _ = STORE.set(Arc::new(store));
            }
        }
    }
    if STORE.get().is_none() && on_vercel() {
        return Err(StoreError(
            "nextrs jobs: no durable store on Vercel — set NEXTRS_JOBS_DB_URL/_TOKEN \
             (or TURSO_DATABASE_URL/_AUTH_TOKEN) and enable the `jobs-libsql` feature, \
             or install one with nextrs::jobs::set_store()"
                .into(),
        ));
    }
    Ok(STORE.get_or_init(|| Arc::new(MemoryJobStore::new())))
}

static LOCAL_ADDR: OnceLock<std::net::SocketAddr> = OnceLock::new();

/// Tell the jobs subsystem the locally bound address, so self-POSTs work in
/// dev where `bind_with_fallback` may not land on `$PORT`. One line in the
/// app's `main` after binding; a no-op for apps without jobs.
pub fn announce_local_addr(addr: std::net::SocketAddr) {
    let _ = LOCAL_ADDR.set(addr);
}

/// Where this deployment answers HTTP. `NEXTRS_BASE_URL` → Vercel's
/// deployment host → the announced local addr → localhost best-effort.
pub(crate) fn base_url() -> String {
    if let Ok(url) = std::env::var("NEXTRS_BASE_URL") {
        return url.trim_end_matches('/').to_string();
    }
    if let Ok(host) = std::env::var("VERCEL_URL") {
        if !host.is_empty() {
            return format!("https://{host}");
        }
    }
    if let Some(addr) = LOCAL_ADDR.get() {
        return format!("http://{addr}");
    }
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".into());
    format!("http://127.0.0.1:{port}")
}

// -------------------------------------------------------------------- secret

static DEV_SECRET: OnceLock<String> = OnceLock::new();

/// The shared secret job routes require. Env `NEXTRS_JOBS_SECRET` when set;
/// locally (not Vercel) a random per-process fallback — self-POSTs from this
/// process carry it, outsiders can't guess it. On Vercel with the env unset:
/// `None` (fail closed; a per-process secret can't cross instances there).
pub(crate) fn jobs_secret() -> Option<String> {
    if let Ok(s) = std::env::var("NEXTRS_JOBS_SECRET") {
        if !s.is_empty() {
            return Some(s);
        }
    }
    if on_vercel() {
        return None;
    }
    Some(
        DEV_SECRET
            .get_or_init(|| {
                let JobId(hex) = generate_job_id();
                format!("dev-{hex}")
            })
            .clone(),
    )
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Is this request allowed to hit the jobs endpoints? Accepts the jobs secret
/// header, or `Authorization: Bearer $CRON_SECRET` (so a Vercel cron pointed
/// at the sweep works with the platform's own convention). Fail-closed.
pub(crate) fn authorized(headers: &http::HeaderMap) -> bool {
    if let Some(secret) = jobs_secret() {
        if headers
            .get(JOBS_SECRET_HEADER)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|got| constant_time_eq(got.as_bytes(), secret.as_bytes()))
        {
            return true;
        }
    }
    if let Ok(cron) = std::env::var("CRON_SECRET") {
        if !cron.is_empty()
            && headers
                .get(http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .is_some_and(|got| constant_time_eq(got.as_bytes(), cron.as_bytes()))
        {
            return true;
        }
    }
    false
}

// ------------------------------------------------------------------- enqueue

/// Wire body of the kick-off/sweep POST. The payload stays in the row — the
/// route reads it back at claim time, so the DB is authoritative.
#[derive(Serialize, Deserialize)]
pub(crate) struct JobEnvelope {
    pub id: JobId,
}

/// Persist a job row and kick off its run route. Called by the
/// macro-generated enqueue wrappers — not usually by hand.
pub async fn enqueue(
    name: &str,
    payload: serde_json::Value,
    max_attempts: u32,
) -> Result<JobHandle, EnqueueError> {
    if on_vercel() && jobs_secret().is_none() {
        return Err(EnqueueError::Config(
            "nextrs jobs: NEXTRS_JOBS_SECRET must be set on Vercel — job routes are \
             authed and instances need a shared secret to deliver to each other"
                .into(),
        ));
    }
    let store = store().map_err(|e| EnqueueError::Config(e.0))?;
    let id = generate_job_id();
    let now = now_ms();
    store
        .insert(JobRow {
            id: id.clone(),
            name: name.to_string(),
            payload,
            status: JobStatus::Queued,
            attempts: 0,
            max_attempts,
            next_run_at: Some(now),
            last_error: None,
            created_at: now,
            updated_at: now,
        })
        .await?;
    let delivered = match deliver(name, &id).await {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!(job = name, id = %id, error = %err,
                "job kick-off delivery failed; the sweep will deliver it");
            false
        }
    };
    Ok(JobHandle { id, delivered })
}

/// POST a job's envelope to its run route on this deployment.
pub(crate) async fn deliver(name: &str, id: &JobId) -> Result<(), String> {
    let url = format!("{}{}/{}", base_url(), NX_JOBS_PREFIX, name);
    let secret = jobs_secret().ok_or("no jobs secret configured")?;
    let mut req = http_client()
        .post(&url)
        .header(JOBS_SECRET_HEADER, secret)
        .json(&JobEnvelope { id: id.clone() });
    // Protected preview deployments 401 self-requests unless bypassed.
    if let Ok(bypass) = std::env::var("VERCEL_AUTOMATION_BYPASS_SECRET") {
        if !bypass.is_empty() {
            req = req.header("x-vercel-protection-bypass", bypass);
        }
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if resp.status() == reqwest::StatusCode::ACCEPTED {
        Ok(())
    } else {
        Err(format!("job route answered {}", resp.status()))
    }
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("nextrs jobs: could not build the HTTP client")
    })
}

// ------------------------------------------------------------------- running

/// Retry back-off after the `attempts`-th failed attempt:
/// `30s · 2^(attempts-1)`, capped at 1 hour.
pub(crate) fn backoff_ms(attempts: u32) -> i64 {
    let base: i64 = 30_000;
    let shifted = base.saturating_mul(1i64 << (attempts.saturating_sub(1)).min(20));
    shifted.min(3_600_000)
}

/// Run one claimed job to completion and record the outcome. This is the
/// future the job route hands to `WaitUntil` — the response has already gone
/// out when it runs.
pub(crate) async fn run_and_record(
    entry: &crate::conventions::JobEntry,
    row: JobRow,
    ext: http::Extensions,
) {
    tracing::info!(job = entry.name, id = %row.id, attempt = row.attempts, "job started");
    let started = std::time::Instant::now();
    let outcome = tokio::time::timeout(
        std::time::Duration::from_millis(entry.timeout_ms),
        (entry.run)(row.payload.clone(), ext),
    )
    .await;
    let ms = started.elapsed().as_millis() as u64;
    let error = match outcome {
        Ok(Ok(())) => None,
        Ok(Err(e)) => Some(e),
        Err(_) => Some(format!("timed out after {} ms", entry.timeout_ms)),
    };
    let store = match store() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(job = entry.name, id = %row.id, error = %e,
                "job finished but the store is gone; outcome not recorded");
            return;
        }
    };
    match error {
        None => {
            if let Err(e) = store.mark_succeeded(&row.id).await {
                tracing::error!(job = entry.name, id = %row.id, error = %e, "mark_succeeded failed");
            }
            tracing::info!(job = entry.name, id = %row.id, attempt = row.attempts, ms, "job succeeded");
        }
        Some(err) => {
            let next = if row.attempts < row.max_attempts {
                Some(now_ms() + backoff_ms(row.attempts))
            } else {
                None
            };
            if let Err(e) = store.mark_failed(&row.id, &err, next).await {
                tracing::error!(job = entry.name, id = %row.id, error = %e, "mark_failed failed");
            }
            match next {
                Some(at) => tracing::error!(
                    job = entry.name, id = %row.id, attempt = row.attempts, ms,
                    error = %err, retry_at_ms = at, "job failed; will retry"
                ),
                None => tracing::error!(
                    job = entry.name, id = %row.id, attempt = row.attempts, ms,
                    error = %err, "job dead: max attempts exhausted"
                ),
            }
        }
    }
}

/// Sweep pass: reclaim stale `running` rows, then re-deliver due rows.
/// `max_timeout_ms` is the largest registered job timeout — the stale cutoff
/// is `now - (max_timeout + 60s)` so no live run gets reclaimed.
pub(crate) async fn sweep(max_timeout_ms: u64, limit: u32) -> Result<serde_json::Value, StoreError> {
    let store = store()?;
    let now = now_ms();
    let reclaimed = store
        .reclaim_stale(now - (max_timeout_ms as i64) - 60_000)
        .await?;
    let due = store.due(now, limit).await?;
    let mut dispatched = 0u32;
    for row in &due {
        match deliver(&row.name, &row.id).await {
            Ok(()) => dispatched += 1,
            Err(err) => {
                tracing::warn!(job = %row.name, id = %row.id, error = %err, "sweep delivery failed")
            }
        }
    }
    Ok(serde_json::json!({
        "reclaimed": reclaimed,
        "due": due.len(),
        "dispatched": dispatched,
    }))
}

/// Status snapshot for `GET /__nx/jobs`: counts by status plus recent rows.
pub(crate) async fn status_snapshot(limit: u32) -> Result<serde_json::Value, StoreError> {
    let recent = store()?.recent(limit).await?;
    let mut counts: HashMap<&'static str, u32> = HashMap::new();
    for row in &recent {
        *counts.entry(row.status.as_str()).or_default() += 1;
    }
    Ok(serde_json::json!({ "counts": counts, "recent": recent }))
}

// -------------------------------------------------------------- axum handlers

/// `POST /__nx/jobs/<name>` — the run route the router mounts per job.
/// Claims the row, hands the work to this request's `WaitUntil`, answers 202
/// before the job body runs. `jobs`/`idx` instead of `&JobEntry` because the
/// background future must own its entry (`'static`).
pub(crate) async fn handle_run(
    jobs: Arc<Vec<crate::conventions::JobEntry>>,
    idx: usize,
    req: http::Request<axum::body::Body>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    use http::StatusCode;

    if !authorized(req.headers()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    // The request went through the app's full layer stack, so this WaitUntil
    // is the platform-backed one on Vercel (tokio::spawn fallback locally),
    // and the extensions carry the app's `Extension<T>` state for the job.
    let wait = req
        .extensions()
        .get::<crate::WaitUntil>()
        .cloned()
        .unwrap_or_default();
    let ext = req.extensions().clone();

    let body = match axum::body::to_bytes(req.into_body(), 64 * 1024).await {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let env: JobEnvelope = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("bad envelope: {e}")).into_response(),
    };

    let store = match store() {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let row = match store.claim(&env.id).await {
        Ok(Some(row)) => row,
        // Already running/terminal or unknown — the double-delivery guard.
        Ok(None) => return (StatusCode::CONFLICT, "job not claimable").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    if row.name != jobs[idx].name {
        // A row delivered to the wrong route; put it back in the queue.
        let _ = store
            .mark_failed(&row.id, "delivered to wrong job route", Some(now_ms()))
            .await;
        return (StatusCode::CONFLICT, "job name mismatch").into_response();
    }

    let accepted = axum::Json(serde_json::json!({ "id": row.id, "attempt": row.attempts }));
    wait.wait_until(async move { run_and_record(&jobs[idx], row, ext).await });
    (StatusCode::ACCEPTED, accepted).into_response()
}

/// `GET|POST /__nx/jobs/sweep` — reclaim stale runs, re-deliver due rows.
/// Point any cron here; `Authorization: Bearer $CRON_SECRET` also passes.
pub(crate) async fn handle_sweep(
    max_timeout_ms: u64,
    req: http::Request<axum::body::Body>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    use http::StatusCode;
    if !authorized(req.headers()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match sweep(max_timeout_ms, 50).await {
        Ok(summary) => axum::Json(summary).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// `GET /__nx/jobs` — authed status JSON: counts + recent rows.
pub(crate) async fn handle_status(
    req: http::Request<axum::body::Body>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    use http::StatusCode;
    if !authorized(req.headers()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match status_snapshot(50).await {
        Ok(snapshot) => axum::Json(snapshot).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[cfg(feature = "jobs-libsql")]
mod libsql_store;
#[cfg(feature = "jobs-libsql")]
pub use libsql_store::LibsqlJobStore;

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str) -> JobRow {
        JobRow {
            id: JobId(id.into()),
            name: "t".into(),
            payload: serde_json::json!({"x": 1}),
            status: JobStatus::Queued,
            attempts: 0,
            max_attempts: 3,
            next_run_at: Some(now_ms()),
            last_error: None,
            created_at: now_ms(),
            updated_at: now_ms(),
        }
    }

    #[tokio::test]
    async fn memory_store_lifecycle() {
        let s = MemoryJobStore::new();
        s.insert(row("a")).await.unwrap();

        // Claim: queued → running, attempts bumped; second claim loses.
        let claimed = s.claim(&JobId("a".into())).await.unwrap().unwrap();
        assert_eq!(claimed.status, JobStatus::Running);
        assert_eq!(claimed.attempts, 1);
        assert!(s.claim(&JobId("a".into())).await.unwrap().is_none());

        // Retryable failure → failed + due; claimable again.
        s.mark_failed(&JobId("a".into()), "boom", Some(now_ms() - 1))
            .await
            .unwrap();
        let due = s.due(now_ms(), 10).await.unwrap();
        assert_eq!(due.len(), 1);
        let again = s.claim(&JobId("a".into())).await.unwrap().unwrap();
        assert_eq!(again.attempts, 2);

        // Terminal failure → dead; not claimable, not due.
        s.mark_failed(&JobId("a".into()), "boom", None).await.unwrap();
        assert!(s.claim(&JobId("a".into())).await.unwrap().is_none());
        assert!(s.due(now_ms(), 10).await.unwrap().is_empty());
        let got = s.get(&JobId("a".into())).await.unwrap().unwrap();
        assert_eq!(got.status, JobStatus::Dead);
        assert_eq!(got.last_error.as_deref(), Some("boom"));

        // Success path on a fresh row.
        s.insert(row("b")).await.unwrap();
        s.claim(&JobId("b".into())).await.unwrap().unwrap();
        s.mark_succeeded(&JobId("b".into())).await.unwrap();
        let got = s.get(&JobId("b".into())).await.unwrap().unwrap();
        assert_eq!(got.status, JobStatus::Succeeded);
        assert_eq!(got.next_run_at, None);
    }

    #[tokio::test]
    async fn reclaim_stale_revives_dead_instances_only() {
        let s = MemoryJobStore::new();
        s.insert(row("a")).await.unwrap();
        s.claim(&JobId("a".into())).await.unwrap().unwrap();
        // Not stale yet — cutoff in the past.
        assert_eq!(s.reclaim_stale(now_ms() - 10_000).await.unwrap(), 0);
        // Stale — cutoff after its updated_at.
        assert_eq!(s.reclaim_stale(now_ms() + 10_000).await.unwrap(), 1);
        let got = s.get(&JobId("a".into())).await.unwrap().unwrap();
        assert_eq!(got.status, JobStatus::Failed);
        assert!(got.next_run_at.is_some());
    }

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(backoff_ms(1), 30_000);
        assert_eq!(backoff_ms(2), 60_000);
        assert_eq!(backoff_ms(3), 120_000);
        assert_eq!(backoff_ms(7), 1_920_000);
        assert_eq!(backoff_ms(8), 3_600_000); // capped
        assert_eq!(backoff_ms(200), 3_600_000); // shift stays sane
    }

    #[test]
    fn job_ids_are_unique_and_hex() {
        let a = generate_job_id();
        let b = generate_job_id();
        assert_ne!(a, b);
        assert_eq!(a.0.len(), 32);
        assert!(a.0.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn constant_time_eq_basics() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }

    #[test]
    fn authorized_fail_closed_without_headers() {
        // Whatever the env, a request with no credentials is refused.
        assert!(!authorized(&http::HeaderMap::new()));
    }
}
