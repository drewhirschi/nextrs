//! First-party background jobs.
//!
//! A job is a `pub async fn` in `app/jobs/<name>/job.rs` annotated with
//! `#[nextrs::job]`. Calling that function from app code does not run the
//! body — the macro re-emits the name as a typed *enqueue* wrapper that
//! persists a job row and then *delivers* it to a runner, one of two ways:
//!
//! - **Vercel Queues** (on Vercel, when the deployment has a consumer
//!   function — see [`vercel_queues`]): the job's envelope is sent to a queue
//!   topic; the consumer function runs the body to completion inside the
//!   delivery request, the queue redelivers a run whose instance died, and a
//!   failed attempt is retried by a delayed message. No secret, no sweep.
//! - **HTTP** (local dev, other hosts, Vercel apps without a consumer): the
//!   wrapper POSTs the job's own reserved route (`/__nx/jobs/<name>`) on this
//!   deployment. That route claims the row, answers `202` immediately, and
//!   runs the real body behind the request's [`WaitUntil`](crate::WaitUntil)
//!   — platform-backed on Vercel, `tokio::spawn` locally. The authed sweep
//!   route redelivers due and stuck rows.
//!
//! `NEXTRS_JOBS_DELIVERY=http|vercel-queues` overrides the choice.
//!
//! Either way the body runs with a per-job timeout. `Err`/timeout mark the row failed with
//! exponential back-off (`backoff_secs · 2^(n-1)`, default 30s, capped at
//! `max_backoff_secs`, default 1h) until `max_attempts`, then `dead`. Every
//! attempt is kept in the row's `history` — duration, error, and the
//! `tracing` lines it logged (with the [`crate::logs`] layer installed) — and
//! a successful job's return value is stored as `result`. The authed sweep route (`/__nx/jobs/sweep`) re-delivers due
//! rows; drive it from any cron.
//!
//! Durability lives in the row, not the kick-off POST: enqueue still returns
//! `Ok` when delivery fails (the sweep picks the row up), and double delivery
//! is harmless because [`JobStore::claim`] is atomic.
//!
//! Storage resolves lazily, once per process: an explicit [`set_store`] wins;
//! else `NEXTRS_JOBS_DB_URL`/`NEXTRS_JOBS_DB_TOKEN` (falling back to
//! `NEXTRS_DB_URL`/`TURSO_DATABASE_URL` and the matching tokens) selects the
//! libsql store (feature `libsql`); else the in-memory store. The in-memory
//! store is never used on Vercel, where instances share nothing. With no
//! store there:
//!
//! - queue delivery carries the whole job in its message instead of a row
//!   (payload, attempt number). It runs and retries the same, but there is
//!   no status, history or Retry — see [`vercel_queues`].
//! - HTTP delivery fails loud at enqueue, as does a missing
//!   `NEXTRS_JOBS_SECRET`: never a silently non-durable queue in production.

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
    /// A succeeded job's return value (`null` for `()` jobs).
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    /// Finished attempts, oldest first (the newest [`MAX_HISTORY`] are kept).
    #[serde(default)]
    pub history: Vec<JobAttempt>,
}

/// Attempts kept per row; older ones are dropped from `history`.
pub const MAX_HISTORY: usize = 20;

/// One finished attempt of a job.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct JobAttempt {
    /// 1-based attempt number.
    pub n: u32,
    /// Unix ms when the attempt started.
    pub started_at: i64,
    pub ms: u64,
    /// `None` when the attempt succeeded.
    pub error: Option<String>,
    /// `tracing` lines the attempt logged (needs the `logs` layer installed).
    #[serde(default)]
    pub lines: Vec<crate::logs::LogLine>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub dropped_lines: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Append `attempt` to `history`, keeping the newest [`MAX_HISTORY`].
pub(crate) fn push_history(history: &mut Vec<JobAttempt>, attempt: JobAttempt) {
    history.push(attempt);
    if history.len() > MAX_HISTORY {
        let excess = history.len() - MAX_HISTORY;
        history.drain(..excess);
    }
}

/// Filter for [`JobStore::list`]. Empty = newest rows.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct JobQuery {
    pub status: Option<JobStatus>,
    pub name: Option<String>,
    /// Page backwards: rows created strictly before this unix ms.
    pub before: Option<i64>,
    pub limit: Option<u32>,
}

impl JobQuery {
    pub(crate) fn limit(&self) -> u32 {
        self.limit.unwrap_or(50).min(500)
    }
    fn matches(&self, r: &JobRow) -> bool {
        self.status.is_none_or(|s| r.status == s)
            && self.name.as_ref().is_none_or(|n| &r.name == n)
            && self.before.is_none_or(|b| r.created_at < b)
    }
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
            EnqueueError::Store(e) => write!(f, "job was not stored: {e}"),
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
    /// Read the job's current row (status, attempts, last error). `None`
    /// when there is no row — including a job enqueued without a store on
    /// Vercel, which lives only in its queue message.
    pub async fn status(&self) -> Result<Option<JobRow>, StoreError> {
        match store() {
            Ok(store) => store.get(&self.id).await,
            Err(_) if delivery_mode() == DeliveryMode::VercelQueues => Ok(None),
            Err(e) => Err(e),
        }
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
    /// Record a successful attempt and the job's return value. Both `mark_*`
    /// only apply while the row is still `running` attempt `attempt.n` — a
    /// run that outlived its reclaim can't overwrite a newer attempt.
    fn mark_succeeded(&self, id: &JobId, attempt: JobAttempt, result: serde_json::Value) -> StoreFuture<'_, ()>;
    /// Record a failed attempt (its `error` becomes `last_error`).
    /// `next_run_at: Some(ms)` → retryable `failed`; `None` → terminal `dead`.
    fn mark_failed(&self, id: &JobId, attempt: JobAttempt, next_run_at: Option<i64>) -> StoreFuture<'_, ()>;
    /// Manual retry: a `failed`/`dead`/`succeeded` row goes back to `queued`,
    /// due now. `None` when the row is missing or currently queued/running.
    fn requeue(&self, id: &JobId) -> StoreFuture<'_, Option<JobRow>>;
    /// Delete terminal (`succeeded`/`dead`) rows last touched before `cutoff`.
    fn prune(&self, cutoff: i64) -> StoreFuture<'_, u64>;
    /// Non-terminal rows due at or before `now`, oldest first.
    fn due(&self, now: i64, limit: u32) -> StoreFuture<'_, Vec<JobRow>>;
    /// `running` rows untouched since `cutoff` go back to due-now `failed`
    /// (instance died mid-run) — or to `dead` once their attempts are spent,
    /// so a job that kills its instance can't retry forever. Returns how
    /// many were reclaimed.
    fn reclaim_stale(&self, cutoff: i64) -> StoreFuture<'_, u32>;
    /// [`reclaim_stale`](Self::reclaim_stale) for one row: if `id` is
    /// `running` and untouched since `cutoff`, it goes back to due-now
    /// `failed` (or `dead` once its attempts are spent). Returns whether it
    /// was reclaimed. The queue consumer calls this on a redelivery, with a
    /// cutoff from that job's own timeout.
    ///
    /// The default falls back to `reclaim_stale(cutoff)`, which may also
    /// reclaim other rows that old; stores should override it.
    fn reclaim_one(&self, id: &JobId, cutoff: i64) -> StoreFuture<'_, bool> {
        let _ = id;
        let all = self.reclaim_stale(cutoff);
        Box::pin(async move { all.await.map(|n| n > 0) })
    }
    fn get(&self, id: &JobId) -> StoreFuture<'_, Option<JobRow>>;
    /// Rows matching `query`, newest first — the dashboard's data.
    fn list(&self, query: JobQuery) -> StoreFuture<'_, Vec<JobRow>>;
    /// Recent rows, newest first — the status endpoint's data.
    fn recent(&self, limit: u32) -> StoreFuture<'_, Vec<JobRow>> {
        self.list(JobQuery {
            limit: Some(limit),
            ..Default::default()
        })
    }
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

/// The reclaim transition for one in-memory row; `false` when it is not a
/// stale `running` row.
fn reclaim_row(row: &mut JobRow, cutoff: i64) -> bool {
    if row.status != JobStatus::Running || row.updated_at >= cutoff {
        return false;
    }
    if row.attempts >= row.max_attempts {
        row.status = JobStatus::Dead;
        row.next_run_at = None;
    } else {
        row.status = JobStatus::Failed;
        row.next_run_at = Some(now_ms());
    }
    row.last_error
        .get_or_insert_with(|| "reclaimed: instance died mid-run".into());
    row.updated_at = now_ms();
    true
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
    fn mark_succeeded(&self, id: &JobId, attempt: JobAttempt, result: serde_json::Value) -> StoreFuture<'_, ()> {
        let id = id.clone();
        Box::pin(async move {
            self.with(|rows| {
                if let Some(row) = rows
                    .get_mut(&id.0)
                    .filter(|r| r.status == JobStatus::Running && r.attempts == attempt.n)
                {
                    row.status = JobStatus::Succeeded;
                    row.next_run_at = None;
                    row.result = Some(result);
                    push_history(&mut row.history, attempt);
                    row.updated_at = now_ms();
                }
            })
        })
    }
    fn mark_failed(&self, id: &JobId, attempt: JobAttempt, next_run_at: Option<i64>) -> StoreFuture<'_, ()> {
        let id = id.clone();
        Box::pin(async move {
            self.with(|rows| {
                if let Some(row) = rows
                    .get_mut(&id.0)
                    .filter(|r| r.status == JobStatus::Running && r.attempts == attempt.n)
                {
                    row.status = if next_run_at.is_some() {
                        JobStatus::Failed
                    } else {
                        JobStatus::Dead
                    };
                    row.next_run_at = next_run_at;
                    row.last_error = attempt.error.clone();
                    push_history(&mut row.history, attempt);
                    row.updated_at = now_ms();
                }
            })
        })
    }
    fn requeue(&self, id: &JobId) -> StoreFuture<'_, Option<JobRow>> {
        let id = id.clone();
        Box::pin(async move {
            self.with(|rows| {
                let row = rows.get_mut(&id.0)?;
                if matches!(row.status, JobStatus::Queued | JobStatus::Running) {
                    return None;
                }
                row.status = JobStatus::Queued;
                row.next_run_at = Some(now_ms());
                row.updated_at = now_ms();
                Some(row.clone())
            })
        })
    }
    fn prune(&self, cutoff: i64) -> StoreFuture<'_, u64> {
        Box::pin(async move {
            self.with(|rows| {
                let before = rows.len();
                rows.retain(|_, r| {
                    !(matches!(r.status, JobStatus::Succeeded | JobStatus::Dead) && r.updated_at < cutoff)
                });
                (before - rows.len()) as u64
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
                    if reclaim_row(row, cutoff) {
                        n += 1;
                    }
                }
                n
            })
        })
    }
    fn reclaim_one(&self, id: &JobId, cutoff: i64) -> StoreFuture<'_, bool> {
        let id = id.clone();
        Box::pin(async move {
            self.with(|rows| {
                rows.get_mut(&id.0)
                    .is_some_and(|row| reclaim_row(row, cutoff))
            })
        })
    }
    fn get(&self, id: &JobId) -> StoreFuture<'_, Option<JobRow>> {
        let id = id.clone();
        Box::pin(async move { self.with(|rows| rows.get(&id.0).cloned()) })
    }
    fn list(&self, query: JobQuery) -> StoreFuture<'_, Vec<JobRow>> {
        Box::pin(async move {
            self.with(|rows| {
                let mut all: Vec<JobRow> = rows.values().filter(|r| query.matches(r)).cloned().collect();
                all.sort_by_key(|r| std::cmp::Reverse(r.created_at));
                all.truncate(query.limit() as usize);
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
    #[cfg(feature = "libsql")]
    {
        if STORE.get().is_none() {
            if let Some(store) = libsql_store::from_env() {
                let _ = STORE.set(Arc::new(store));
            }
        }
    }
    if STORE.get().is_none() && on_vercel() {
        return Err(StoreError(
            "nextrs jobs: no durable store on Vercel — set NEXTRS_DB_URL/_TOKEN \
             (or TURSO_DATABASE_URL/_AUTH_TOKEN) and enable the `libsql` feature, \
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
///
/// Off Vercel, with the in-memory store, it also starts an in-process
/// sweeper (every 30s), so locally a job's longer back-offs and stale runs are
/// picked up the way a production cron hitting `/__nx/jobs/sweep` would.
///
/// It does **not** auto-start against a database: a dev server whose env
/// points at a shared (say, production) Turso would otherwise claim and run
/// that deployment's jobs with dev code. `NEXTRS_JOBS_LOCAL_SWEEP=1` opts in
/// (e.g. a single self-hosted server with its own database); `=0` opts out.
pub fn announce_local_addr(addr: std::net::SocketAddr) {
    if LOCAL_ADDR.set(addr).is_err() || on_vercel() {
        return;
    }
    let shared_db = {
        #[cfg(feature = "libsql")]
        {
            crate::db::env_url(&["NEXTRS_JOBS_DB_URL"], &["NEXTRS_JOBS_DB_TOKEN"]).is_some()
        }
        #[cfg(not(feature = "libsql"))]
        {
            false
        }
    };
    match std::env::var("NEXTRS_JOBS_LOCAL_SWEEP").as_deref() {
        Ok("0" | "false" | "off") => return,
        Ok("1" | "true" | "on") => {}
        _ if shared_db => {
            tracing::info!(
                "jobs: local sweeper off (a database is configured; NEXTRS_JOBS_LOCAL_SWEEP=1 to run it)"
            );
            return;
        }
        _ => {}
    }
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        runtime.spawn(async {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
            tick.tick().await; // the first tick is immediate; skip it
            loop {
                tick.tick().await;
                if let Err(e) = sweep(max_timeout_ms(), 50).await {
                    tracing::warn!(error = %e, "local job sweep failed");
                }
            }
        });
    }
}

static MAX_TIMEOUT_MS: OnceLock<u64> = OnceLock::new();

/// Record the slowest registered job's timeout (the router does, at build).
pub(crate) fn set_max_timeout_ms(ms: u64) {
    let _ = MAX_TIMEOUT_MS.set(ms);
}

fn max_timeout_ms() -> u64 {
    MAX_TIMEOUT_MS.get().copied().unwrap_or(60_000)
}

/// Back-offs up to this long are retried by the failed run itself (it waits,
/// then re-delivers) instead of waiting for the next sweep — so a 2s back-off
/// is actually 2s. Longer ones are the sweeper's: holding an instance for
/// minutes would bill for idle time.
const SELF_RETRY_MAX_MS: i64 = 60_000;

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
            .get_or_init(|| format!("dev-{}", crate::os_random_hex()))
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

/// Wire body of the kick-off/sweep POST and of a queue message. The payload
/// stays in the row — the runner reads it back at claim time, so the DB is
/// authoritative. `name` routes a queue message to its job (the HTTP path
/// already has it in the URL).
#[derive(Serialize, Deserialize)]
pub(crate) struct JobEnvelope {
    pub id: JobId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Rowless jobs only (no store configured): the message is the record,
    /// so it carries the payload and where the retries stand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    /// 1-based attempt this message is for (rowless only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_attempts: Option<u32>,
}

impl JobEnvelope {
    /// The envelope of a job that has a row: just where to find it.
    pub(crate) fn for_row(id: &JobId, name: Option<&str>) -> Self {
        Self {
            id: id.clone(),
            name: name.map(str::to_owned),
            payload: None,
            attempt: None,
            max_attempts: None,
        }
    }

    /// The envelope of a rowless job: everything a runner needs.
    pub(crate) fn rowless(
        id: &JobId,
        name: &str,
        payload: serde_json::Value,
        attempt: u32,
        max_attempts: u32,
    ) -> Self {
        Self {
            id: id.clone(),
            name: Some(name.to_string()),
            payload: Some(payload),
            attempt: Some(attempt),
            max_attempts: Some(max_attempts),
        }
    }
}

/// Where a job's record lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecordMode {
    /// A row in the [`JobStore`]: status, history, results, Retry.
    Rows,
    /// No store: the queue message carries the payload and attempt number.
    /// The job runs and retries, but nothing can be looked up afterwards.
    Rowless,
}

/// Pick where a job's record lives. Rows whenever a store is available.
/// Without one (which only happens on Vercel — elsewhere the in-memory store
/// stands in), queue delivery can still carry the job by itself; HTTP
/// delivery cannot, because the sweep and the run route both need the row.
pub(crate) fn select_record(store_available: bool, delivery: DeliveryMode) -> Option<RecordMode> {
    match (store_available, delivery) {
        (true, _) => Some(RecordMode::Rows),
        (false, DeliveryMode::VercelQueues) => Some(RecordMode::Rowless),
        (false, DeliveryMode::Http) => None,
    }
}

/// How a queued row reaches a runner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeliveryMode {
    /// Self-POST to `/__nx/jobs/<name>`; the sweep redelivers.
    Http,
    /// SendMessage to the Vercel Queues topic; the consumer function runs it.
    VercelQueues,
}

/// Pick the delivery mode. `NEXTRS_JOBS_DELIVERY` wins when it names a mode;
/// otherwise queues are used only on Vercel *and* only when the deployment
/// has a consumer function — so a Vercel app that has not added `api/jobs.rs`
/// keeps working exactly as before.
pub(crate) fn select_delivery(
    override_mode: Option<&str>,
    on_vercel: bool,
    consumer_present: bool,
) -> DeliveryMode {
    match override_mode.map(str::trim) {
        Some("http") => DeliveryMode::Http,
        Some("vercel-queues") => DeliveryMode::VercelQueues,
        _ if on_vercel && consumer_present => DeliveryMode::VercelQueues,
        _ => DeliveryMode::Http,
    }
}

pub(crate) fn delivery_mode() -> DeliveryMode {
    select_delivery(
        std::env::var("NEXTRS_JOBS_DELIVERY").ok().as_deref(),
        on_vercel(),
        vercel_queues::consumer_present(),
    )
}

/// Persist a job row and kick off its run route. Called by the
/// macro-generated enqueue wrappers — not usually by hand.
pub async fn enqueue(
    name: &str,
    payload: serde_json::Value,
    max_attempts: u32,
) -> Result<JobHandle, EnqueueError> {
    // Only HTTP delivery needs the shared secret; a queue consumer is not
    // reachable from outside the deployment.
    if on_vercel() && delivery_mode() == DeliveryMode::Http && jobs_secret().is_none() {
        return Err(EnqueueError::Config(
            "nextrs jobs: NEXTRS_JOBS_SECRET must be set on Vercel — job routes are \
             authed and instances need a shared secret to deliver to each other \
             (or deploy the api/jobs.rs queue consumer, which needs no secret)"
                .into(),
        ));
    }
    let id = generate_job_id();
    let store = match store() {
        Ok(store) => store,
        Err(e) => match select_record(false, delivery_mode()) {
            // No store, but a queue: the message itself is the job. A send
            // that fails here loses the job, so it is an error, not
            // `delivered: false`.
            Some(_) => {
                return match vercel_queues::send_rowless(name, &id, payload, max_attempts).await {
                    Ok(()) => Ok(JobHandle {
                        id,
                        delivered: true,
                    }),
                    Err(err) => Err(EnqueueError::Store(StoreError(format!(
                        "no job store is configured and the job's queue message was not sent: {err}"
                    )))),
                };
            }
            None => return Err(EnqueueError::Config(e.0)),
        },
    };
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
            result: None,
            history: Vec::new(),
        })
        .await?;
    let delivered = match deliver(name, &id, None).await {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!(job = name, id = %id, error = %err,
                "job kick-off delivery failed; a sweep or a manual retry will deliver it");
            false
        }
    };
    Ok(JobHandle { id, delivered })
}

/// Hand a queued row to a runner: a message on the jobs topic in queue mode
/// (after `delay`, when given), else a POST to its run route. HTTP delivery
/// has no delay of its own — its callers wait first.
pub(crate) async fn deliver(
    name: &str,
    id: &JobId,
    delay: Option<std::time::Duration>,
) -> Result<(), String> {
    match delivery_mode() {
        DeliveryMode::VercelQueues => vercel_queues::send(name, id, delay).await,
        DeliveryMode::Http => deliver_http(name, id).await,
    }
}

/// POST a job's envelope to its run route on this deployment.
async fn deliver_http(name: &str, id: &JobId) -> Result<(), String> {
    let url = format!("{}{}/{}", base_url(), NX_JOBS_PREFIX, name);
    let secret = jobs_secret().ok_or("no jobs secret configured")?;
    let mut req = http_client()
        .post(&url)
        .header(JOBS_SECRET_HEADER, secret)
        .json(&JobEnvelope::for_row(id, None));
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

pub(crate) fn http_client() -> &'static reqwest::Client {
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
/// `base · 2^(attempts-1)`, capped at `max`.
pub(crate) fn backoff_ms(attempts: u32, base: u64, max: u64) -> i64 {
    let shifted = (base as i64).saturating_mul(1i64 << (attempts.saturating_sub(1)).min(20));
    shifted.min(max as i64)
}

/// The attempt the current task is running, inside a job body.
#[derive(Clone, Debug)]
pub struct CurrentJob {
    pub id: JobId,
    pub name: String,
    /// 1-based.
    pub attempt: u32,
    pub max_attempts: u32,
}

tokio::task_local! {
    static CURRENT: CurrentJob;
}

/// Which job and attempt is running — `None` outside a job body. Handy for
/// attempt-aware behavior and for tagging your own logs.
pub fn current() -> Option<CurrentJob> {
    CURRENT.try_with(Clone::clone).ok()
}

/// How one attempt ended, after it was recorded in the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AttemptOutcome {
    Succeeded,
    /// Failed with attempts left; the row is `failed`, due in `delay_ms`.
    Retry {
        delay_ms: i64,
    },
    /// Failed with no attempts left; the row is `dead`.
    Dead,
    /// The attempt ran but its outcome could not be written to the store.
    Unrecorded,
}

/// Run one claimed job to completion and record the outcome in `store`
/// (`None` for a rowless job: there is nothing to record it in).
/// Delivering the retry is the caller's business — it differs by mode.
pub(crate) async fn run_attempt(
    store: Option<&dyn JobStore>,
    entry: &crate::conventions::JobEntry,
    row: JobRow,
    ext: http::Extensions,
) -> AttemptOutcome {
    tracing::info!(job = entry.name, id = %row.id, attempt = row.attempts, "job started");
    let started_at = now_ms();
    let started = std::time::Instant::now();
    // The attempt's own log capture, and `current()` for the body.
    let capture = crate::logs::Capture::new();
    let current = CurrentJob {
        id: row.id.clone(),
        name: row.name.clone(),
        attempt: row.attempts,
        max_attempts: row.max_attempts,
    };
    let body = tokio::time::timeout(
        std::time::Duration::from_millis(entry.timeout_ms),
        (entry.run)(row.payload.clone(), ext),
    );
    let outcome = crate::logs::scope(Arc::clone(&capture), CURRENT.scope(current, body)).await;
    // WaitUntil work the body started still writes to this attempt's lines.
    capture.wait_idle(std::time::Duration::from_secs(10)).await;
    let ms = started.elapsed().as_millis() as u64;
    let (result, error) = match outcome {
        Ok(Ok(value)) => (Some(value), None),
        Ok(Err(e)) => (None, Some(e)),
        Err(_) => (None, Some(format!("timed out after {} ms", entry.timeout_ms))),
    };
    let attempt = JobAttempt {
        n: row.attempts,
        started_at,
        ms,
        error: error.clone(),
        lines: capture.lines(),
        dropped_lines: capture.dropped(),
    };
    match error {
        None => {
            let result = result.unwrap_or(serde_json::Value::Null);
            let recorded = match store {
                Some(store) => store.mark_succeeded(&row.id, attempt, result).await,
                None => Ok(()),
            };
            if let Err(e) = recorded {
                tracing::error!(job = entry.name, id = %row.id, error = %e, "mark_succeeded failed");
                return AttemptOutcome::Unrecorded;
            }
            tracing::info!(job = entry.name, id = %row.id, attempt = row.attempts, ms, "job succeeded");
            AttemptOutcome::Succeeded
        }
        Some(err) => {
            let delay_ms = (row.attempts < row.max_attempts)
                .then(|| backoff_ms(row.attempts, entry.backoff_ms, entry.max_backoff_ms));
            let next = delay_ms.map(|delay| now_ms() + delay);
            let recorded = match store {
                Some(store) => store.mark_failed(&row.id, attempt, next).await,
                None => Ok(()),
            };
            if let Err(e) = recorded {
                tracing::error!(job = entry.name, id = %row.id, error = %e, "mark_failed failed");
                return AttemptOutcome::Unrecorded;
            }
            match delay_ms {
                Some(delay_ms) => {
                    tracing::error!(
                        job = entry.name, id = %row.id, attempt = row.attempts, ms,
                        error = %err, retry_in_ms = delay_ms, "job failed; will retry"
                    );
                    AttemptOutcome::Retry { delay_ms }
                }
                None => {
                    tracing::error!(
                        job = entry.name, id = %row.id, attempt = row.attempts, ms,
                        error = %err, "job dead: max attempts exhausted"
                    );
                    AttemptOutcome::Dead
                }
            }
        }
    }
}

/// Run one claimed job and deliver its retry. This is the future the HTTP
/// run route hands to `WaitUntil` — the response has already gone out when
/// it runs.
pub(crate) async fn run_and_record(
    entry: &crate::conventions::JobEntry,
    row: JobRow,
    ext: http::Extensions,
) {
    let id = row.id.clone();
    let store = match store() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(job = entry.name, id = %id, error = %e,
                "job claimed but the store is gone; not run");
            return;
        }
    };
    let AttemptOutcome::Retry { delay_ms } =
        run_attempt(Some(store.as_ref()), entry, row, ext).await
    else {
        return;
    };
    let delay = std::time::Duration::from_millis(delay_ms.max(0) as u64);
    let delivered = match delivery_mode() {
        // The queue holds the retry for exactly the back-off.
        DeliveryMode::VercelQueues => deliver(entry.name, &id, Some(delay)).await,
        // Short back-off: this run (still inside its WaitUntil) sleeps and
        // re-delivers. Claiming is atomic, so a sweep that gets there first
        // just wins the race. Longer ones are the sweep's.
        DeliveryMode::Http if delay_ms <= SELF_RETRY_MAX_MS => {
            tokio::time::sleep(delay).await;
            deliver(entry.name, &id, None).await
        }
        DeliveryMode::Http => Ok(()),
    };
    if let Err(e) = delivered {
        tracing::warn!(job = entry.name, id = %id, error = %e,
            "retry delivery failed; the sweep will deliver it");
    }
}

/// Manual retry (the dashboard's Retry button): requeue a finished row and
/// kick it off now. A dead job gets one more attempt; a succeeded job runs
/// again with the same payload. Delivery failure is fine — the sweep runs it.
pub async fn retry(id: &JobId) -> Result<Option<JobRow>, StoreError> {
    let store = store()?;
    let Some(row) = store.requeue(id).await? else {
        return Ok(None);
    };
    if let Err(err) = deliver(&row.name, &row.id, None).await {
        tracing::warn!(job = %row.name, id = %row.id, error = %err, "retry delivery failed; the sweep will deliver it");
    }
    Ok(Some(row))
}

/// Days to keep finished job rows (`NEXTRS_JOBS_RETENTION_DAYS`, default 30).
fn retention_days() -> i64 {
    std::env::var("NEXTRS_JOBS_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30)
}

/// Delete finished job rows past their retention, and expired request logs.
/// Returns `(jobs, logs)` deleted. The sweep does this on every pass; the
/// queue consumer, which has no sweep, once per instance.
pub(crate) async fn prune_expired() -> Result<(u64, u64), StoreError> {
    let pruned = store()?
        .prune(now_ms() - retention_days() * 86_400_000)
        .await?;
    #[cfg(feature = "logs")]
    let pruned_logs = crate::logs::prune_expired().await.unwrap_or(0);
    #[cfg(not(feature = "logs"))]
    let pruned_logs = 0u64;
    Ok((pruned, pruned_logs))
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
    let (pruned, pruned_logs) = prune_expired().await?;
    let due = store.due(now, limit).await?;
    let mut dispatched = 0u32;
    for row in &due {
        match deliver(&row.name, &row.id, None).await {
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
        "pruned": pruned,
        "pruned_logs": pruned_logs,
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

    // A queue delivery, routed here by `vercel_queues::consumer`. The
    // extension can only be set in-process, so it needs no secret; the job
    // runs to completion inside this request.
    if let Some(delivery) = req.extensions().get::<vercel_queues::Delivery>().cloned() {
        return vercel_queues::handle_delivery(&jobs[idx], delivery, req).await;
    }
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
            .mark_failed(
                &row.id,
                JobAttempt {
                    n: row.attempts,
                    started_at: now_ms(),
                    ms: 0,
                    error: Some("delivered to wrong job route".into()),
                    lines: Vec::new(),
                    dropped_lines: 0,
                },
                Some(now_ms()),
            )
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

pub mod vercel_queues;

#[cfg(feature = "libsql")]
mod libsql_store;
#[cfg(feature = "libsql")]
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
            result: None,
            history: Vec::new(),
        }
    }

    fn attempt(n: u32, error: Option<&str>) -> JobAttempt {
        JobAttempt {
            n,
            started_at: now_ms(),
            ms: 1,
            error: error.map(Into::into),
            lines: Vec::new(),
            dropped_lines: 0,
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
        s.mark_failed(&JobId("a".into()), attempt(1, Some("boom")), Some(now_ms() - 1))
            .await
            .unwrap();
        let due = s.due(now_ms(), 10).await.unwrap();
        assert_eq!(due.len(), 1);
        let again = s.claim(&JobId("a".into())).await.unwrap().unwrap();
        assert_eq!(again.attempts, 2);

        // Terminal failure → dead; not claimable, not due.
        s.mark_failed(&JobId("a".into()), attempt(2, Some("boom")), None).await.unwrap();
        assert!(s.claim(&JobId("a".into())).await.unwrap().is_none());
        assert!(s.due(now_ms(), 10).await.unwrap().is_empty());
        let got = s.get(&JobId("a".into())).await.unwrap().unwrap();
        assert_eq!(got.status, JobStatus::Dead);
        assert_eq!(got.last_error.as_deref(), Some("boom"));
        assert_eq!(got.history.iter().map(|a| a.n).collect::<Vec<_>>(), [1, 2]);

        // Manual retry: dead → queued and due; a queued row can't be requeued.
        let requeued = s.requeue(&JobId("a".into())).await.unwrap().unwrap();
        assert_eq!(requeued.status, JobStatus::Queued);
        assert!(s.requeue(&JobId("a".into())).await.unwrap().is_none());

        // Success path on a fresh row.
        s.insert(row("b")).await.unwrap();
        s.claim(&JobId("b".into())).await.unwrap().unwrap();
        s.mark_succeeded(&JobId("b".into()), attempt(1, None), serde_json::json!({"sent": true}))
            .await
            .unwrap();
        let got = s.get(&JobId("b".into())).await.unwrap().unwrap();
        assert_eq!(got.status, JobStatus::Succeeded);
        assert_eq!(got.next_run_at, None);
        assert_eq!(got.result, Some(serde_json::json!({"sent": true})));

        // Filtered listing + pruning of old terminal rows.
        let succeeded = s
            .list(JobQuery { status: Some(JobStatus::Succeeded), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(succeeded.len(), 1);
        assert_eq!(s.prune(now_ms() + 1).await.unwrap(), 1);
        assert!(s.get(&JobId("b".into())).await.unwrap().is_none());
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

    #[tokio::test]
    async fn a_job_that_keeps_killing_its_instance_ends_dead() {
        let s = MemoryJobStore::new();
        let mut r = row("crash");
        r.max_attempts = 1;
        s.insert(r).await.unwrap();
        s.claim(&JobId("crash".into())).await.unwrap().unwrap();
        // The instance died mid-run: no mark_*; the sweep reclaims it.
        assert_eq!(s.reclaim_stale(now_ms() + 10_000).await.unwrap(), 1);
        let got = s.get(&JobId("crash".into())).await.unwrap().unwrap();
        assert_eq!(got.status, JobStatus::Dead, "attempts spent → dead, not retried forever");
        assert_eq!(got.next_run_at, None);
    }

    #[tokio::test]
    async fn a_zombie_run_cannot_overwrite_a_newer_attempt() {
        let s = MemoryJobStore::new();
        s.insert(row("z")).await.unwrap();
        s.claim(&JobId("z".into())).await.unwrap().unwrap(); // attempt 1
        s.reclaim_stale(now_ms() + 10_000).await.unwrap();
        s.claim(&JobId("z".into())).await.unwrap().unwrap(); // attempt 2 running
        // Attempt 1 finally finishes and reports — it must be ignored.
        s.mark_succeeded(&JobId("z".into()), attempt(1, None), serde_json::json!("stale"))
            .await
            .unwrap();
        let got = s.get(&JobId("z".into())).await.unwrap().unwrap();
        assert_eq!((got.status, got.attempts, got.result), (JobStatus::Running, 2, None));
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let (base, max) = (30_000, 3_600_000);
        assert_eq!(backoff_ms(1, base, max), 30_000);
        assert_eq!(backoff_ms(2, base, max), 60_000);
        assert_eq!(backoff_ms(3, base, max), 120_000);
        assert_eq!(backoff_ms(7, base, max), 1_920_000);
        assert_eq!(backoff_ms(8, base, max), 3_600_000); // capped
        assert_eq!(backoff_ms(200, base, max), 3_600_000); // shift stays sane
        assert_eq!(backoff_ms(3, 2_000, 60_000), 8_000); // custom base
    }

    #[test]
    fn delivery_mode_is_queues_only_on_vercel_with_a_consumer() {
        use DeliveryMode::{Http, VercelQueues};
        // No override: both conditions are needed.
        assert_eq!(select_delivery(None, true, true), VercelQueues);
        assert_eq!(
            select_delivery(None, true, false),
            Http,
            "Vercel app without api/jobs.rs"
        );
        assert_eq!(
            select_delivery(None, false, true),
            Http,
            "local dev / other hosts"
        );
        assert_eq!(select_delivery(None, false, false), Http);
        // The override wins either way.
        assert_eq!(select_delivery(Some("http"), true, true), Http);
        assert_eq!(
            select_delivery(Some("vercel-queues"), false, false),
            VercelQueues
        );
        assert_eq!(
            select_delivery(Some(" vercel-queues "), false, false),
            VercelQueues
        );
        // An unknown value is ignored rather than guessed at.
        assert_eq!(select_delivery(Some("sqs"), true, true), VercelQueues);
        assert_eq!(select_delivery(Some(""), true, false), Http);
    }

    #[test]
    fn envelope_reads_the_http_shape_and_writes_the_queue_shape() {
        let http: JobEnvelope = serde_json::from_str(r#"{"id":"a"}"#).unwrap();
        assert_eq!((http.id.0.as_str(), http.name.as_deref()), ("a", None));
        assert_eq!(serde_json::to_string(&http).unwrap(), r#"{"id":"a"}"#);
        let queue = JobEnvelope::for_row(&JobId("a".into()), Some("audit-todo"));
        assert_eq!(
            serde_json::to_string(&queue).unwrap(),
            r#"{"id":"a","name":"audit-todo"}"#
        );
        // A rowless message carries the whole job.
        let rowless = JobEnvelope::rowless(
            &JobId("a".into()),
            "audit-todo",
            serde_json::json!({"x": 1}),
            2,
            3,
        );
        assert_eq!(
            serde_json::to_value(&rowless).unwrap(),
            serde_json::json!({
                "id": "a", "name": "audit-todo", "payload": {"x": 1}, "attempt": 2, "max_attempts": 3
            })
        );
    }

    #[test]
    fn jobs_are_rowless_only_without_a_store_and_with_a_queue() {
        use DeliveryMode::{Http, VercelQueues};
        // A store always wins: history, status and Retry are worth having.
        assert_eq!(select_record(true, VercelQueues), Some(RecordMode::Rows));
        assert_eq!(select_record(true, Http), Some(RecordMode::Rows));
        assert_eq!(
            select_record(false, VercelQueues),
            Some(RecordMode::Rowless)
        );
        // HTTP delivery has nothing to carry the job: still a loud error.
        assert_eq!(select_record(false, Http), None);
    }

    #[test]
    fn history_keeps_the_newest_attempts() {
        let mut h = Vec::new();
        for n in 1..=(MAX_HISTORY as u32 + 3) {
            push_history(&mut h, attempt(n, None));
        }
        assert_eq!(h.len(), MAX_HISTORY);
        assert_eq!(h.first().unwrap().n, 4);
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
