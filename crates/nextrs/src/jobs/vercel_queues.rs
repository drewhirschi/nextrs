//! Vercel Queues as the delivery mechanism for jobs.
//!
//! On Vercel, when the deployment has a consumer function, a job is delivered
//! by sending its envelope (`{ id, name }`) to one queue topic ([`TOPIC`])
//! instead of POSTing the job's run route. The job row in the
//! [`JobStore`](super::JobStore) is still the record; the queue only decides
//! *when a runner sees it* and delivers it again if the runner dies.
//!
//! The consumer is a second Vercel function, `api/jobs.rs`:
//!
//! ```ignore
//! use nextrs::vercel::StreamingVercelLayer;
//! use tower::ServiceBuilder;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), vercel_runtime::Error> {
//!     let app = ServiceBuilder::new()
//!         .layer(StreamingVercelLayer::new())
//!         .service(nextrs::jobs::vercel_queues::consumer(my_app::app()));
//!     vercel_runtime::run(app).await
//! }
//! ```
//!
//! with a `queue/v2beta` trigger for the topic in its function config
//! (`nextrs generate` / `nextrs deploy` write it). Vercel calls the function
//! with one HTTP request per delivery: the body is the message, the metadata
//! is in `ce-*` headers. For each one the consumer
//!
//! 1. looks at the row and decides ([`redelivery_decision`]): run it, reclaim
//!    a stale `running` row first, leave it alone, or drop the message;
//! 2. runs the job **to completion inside the request** and records the
//!    outcome;
//! 3. on a retryable failure sends a new message with `Vqs-Delay-Seconds` set
//!    to the job's back-off;
//! 4. acknowledges the delivery with its receipt handle. A 2xx response does
//!    not acknowledge a message, so this call is what ends it.
//!
//! When something goes wrong before the outcome is safe (the store is down,
//! the retry message could not be sent) the delivery is *not* acknowledged,
//! and the queue delivers it again after its visibility timeout.
//!
//! A consumer function is not reachable from outside the deployment, so this
//! path needs no `NEXTRS_JOBS_SECRET` and no cron-driven sweep.
//!
//! # Without a store
//!
//! With no [`JobStore`](super::JobStore) configured on Vercel there is no row
//! to point at, so the message carries the whole job instead: payload,
//! attempt number, retry budget. The consumer runs it and sends attempt
//! `n + 1` as a new delayed message when it fails. Retries and back-off work
//! the same; what is gone is everything that reads a row — status, history,
//! stored results, the admin pages, Retry — and the claim that stops a
//! redelivered message from running a job twice. Configure a store and jobs
//! enqueued from then on get rows again.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use super::{AttemptOutcome, JobEnvelope, JobId, JobRow, JobStatus, JobStore, now_ms};

/// The one queue topic every job's envelope is sent to.
pub const TOPIC: &str = "nx-jobs";

/// `ce-type` of a Vercel Queues push delivery.
pub const CE_TYPE: &str = "com.vercel.queue.v2beta";

/// Set by `nextrs deploy` in every function's environment when the
/// deployment has a consumer function for [`TOPIC`].
pub const CONSUMER_ENV: &str = "NEXTRS_JOBS_CONSUMER";

/// Vercel Queues accepts a delivery delay of at most seven days.
const MAX_DELAY_SECS: u64 = 7 * 24 * 60 * 60;

/// Largest delivery body the consumer reads. A row-backed message is a few
/// dozen bytes; a rowless one carries the job's payload.
const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

/// A `running` row is stale once it has been untouched for its job's timeout
/// plus this much (the same slack the sweep uses).
const STALE_GRACE_MS: i64 = 60_000;

// ------------------------------------------------------------- process state

static IS_CONSUMER: AtomicBool = AtomicBool::new(false);

/// Does this deployment have a consumer function? True inside the consumer
/// itself, and in any function `nextrs deploy` marked with [`CONSUMER_ENV`].
pub(crate) fn consumer_present() -> bool {
    IS_CONSUMER.load(Ordering::Relaxed)
        || std::env::var(CONSUMER_ENV).is_ok_and(|v| v == "vercel-queues")
}

static OIDC_TOKEN: RwLock<Option<String>> = RwLock::new(None);

/// Remember the OIDC token Vercel put on this request. At runtime the token
/// arrives per request (`x-vercel-oidc-token`), not in the environment, and
/// work that outlives the request still needs one to send.
pub(crate) fn note_request(headers: &http::HeaderMap) {
    let Some(token) = headers
        .get("x-vercel-oidc-token")
        .and_then(|v| v.to_str().ok())
        .filter(|t| !t.is_empty())
    else {
        return;
    };
    if OIDC_TOKEN
        .read()
        .is_ok_and(|current| current.as_deref() == Some(token))
    {
        return;
    }
    if let Ok(mut current) = OIDC_TOKEN.write() {
        *current = Some(token.to_string());
    }
}

/// Router middleware: keep the newest request's OIDC token for sends.
pub(crate) async fn remember_oidc_token(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    note_request(req.headers());
    next.run(req).await
}

fn oidc_token() -> Option<String> {
    if let Some(token) = OIDC_TOKEN.read().ok().and_then(|t| t.clone()) {
        return Some(token);
    }
    std::env::var("VERCEL_OIDC_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())
}

// ------------------------------------------------------------------ delivery

/// One push delivery, read from the request's CloudEvents headers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivery {
    pub message_id: String,
    pub topic: String,
    pub consumer_group: String,
    pub receipt_handle: String,
    /// Starts at 1. Not the job's attempt number — that is the row's.
    pub delivery_count: u32,
    pub region: Option<String>,
}

/// Read a delivery from `ce-*` headers. `Err` says which header is wrong, so
/// a request that is not a queue delivery is refused with a reason.
pub(crate) fn parse_delivery(headers: &http::HeaderMap) -> Result<Delivery, String> {
    let get = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|v| !v.is_empty())
    };
    match get("ce-type") {
        Some(CE_TYPE) => {}
        Some(other) => return Err(format!("unsupported ce-type {other:?}")),
        None => return Err("not a queue delivery (no ce-type header)".into()),
    }
    let required = |name: &str| {
        get(name)
            .map(str::to_owned)
            .ok_or(format!("missing {name}"))
    };
    Ok(Delivery {
        message_id: required("ce-vqsmessageid")?,
        topic: required("ce-vqsqueuename")?,
        consumer_group: required("ce-vqsconsumergroup")?,
        receipt_handle: required("ce-vqsreceipthandle")?,
        delivery_count: get("ce-vqsdeliverycount")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1),
        region: get("ce-vqsregion").map(str::to_owned),
    })
}

/// `Vqs-Delay-Seconds` for a back-off: whole seconds, rounded up so a retry
/// is never early, clamped to the queue's seven-day limit. `None` = send now.
pub(crate) fn delay_seconds(delay_ms: i64) -> Option<u64> {
    if delay_ms <= 0 {
        return None;
    }
    Some((delay_ms as u64).div_ceil(1000).min(MAX_DELAY_SECS))
}

/// What to do with a delivery, given the row it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Redelivery {
    /// `queued` or `failed`: claim it and run.
    Run,
    /// `running` but untouched for longer than the job's timeout: the
    /// instance that held it died. Reclaim it, then run (or it goes `dead`
    /// when its attempts are spent).
    Reclaim,
    /// `running` and fresh: another delivery is running it right now. Do not
    /// acknowledge — if that run dies, this message is what brings it back.
    Leave,
    /// Finished, or no such row: nothing to do; acknowledge.
    Drop,
}

/// Decide what a delivery means for its row. A row is only ever re-run on a
/// *redelivery* when its run has outlived the job's own timeout, so a slow
/// job is not started twice.
pub(crate) fn redelivery_decision(row: Option<&JobRow>, now: i64, timeout_ms: u64) -> Redelivery {
    let Some(row) = row else {
        return Redelivery::Drop;
    };
    match row.status {
        JobStatus::Queued | JobStatus::Failed => Redelivery::Run,
        JobStatus::Succeeded | JobStatus::Dead => Redelivery::Drop,
        JobStatus::Running => {
            if row.updated_at < stale_cutoff(now, timeout_ms) {
                Redelivery::Reclaim
            } else {
                Redelivery::Leave
            }
        }
    }
}

fn stale_cutoff(now: i64, timeout_ms: u64) -> i64 {
    now - (timeout_ms as i64) - STALE_GRACE_MS
}

// -------------------------------------------------------------------- client

/// The two REST calls the jobs backend needs: SendMessage and Acknowledge.
#[derive(Clone, Debug)]
pub(crate) struct QueueClient {
    /// `https://{region}.vercel-queue.com`, no trailing slash.
    base: String,
    token: String,
    /// Messages are scoped to a deployment: without this header a message is
    /// accepted but never reaches a preview deployment's consumers.
    deployment_id: Option<String>,
}

impl QueueClient {
    pub(crate) fn new(base: &str, token: &str, deployment_id: Option<&str>) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            token: token.to_string(),
            deployment_id: deployment_id.map(str::to_owned),
        }
    }

    /// The client for this deployment. Messages do not cross regions, so the
    /// endpoint is the region of the delivery being handled, else the
    /// function's own (`VERCEL_REGION`). `NEXTRS_VERCEL_QUEUE_URL` replaces
    /// the endpoint (tests, a proxy).
    pub(crate) fn from_env(region: Option<&str>) -> Result<Self, String> {
        let token = oidc_token().ok_or(
            "no Vercel OIDC token: neither an x-vercel-oidc-token request header nor VERCEL_OIDC_TOKEN",
        )?;
        let base = match std::env::var("NEXTRS_VERCEL_QUEUE_URL") {
            Ok(url) if !url.is_empty() => url,
            _ => {
                let region = region
                    .map(str::to_owned)
                    .or_else(|| {
                        std::env::var("VERCEL_REGION")
                            .ok()
                            .filter(|r| !r.is_empty())
                    })
                    .ok_or("no queue region: VERCEL_REGION is not set")?;
                format!("https://{region}.vercel-queue.com")
            }
        };
        let deployment_id = std::env::var("VERCEL_DEPLOYMENT_ID")
            .ok()
            .filter(|d| !d.is_empty());
        Ok(Self::new(&base, &token, deployment_id.as_deref()))
    }

    fn url(&self, segments: &[&str]) -> Result<reqwest::Url, String> {
        let mut url = reqwest::Url::parse(&self.base).map_err(|e| format!("bad queue URL: {e}"))?;
        url.path_segments_mut()
            .map_err(|()| "bad queue URL: cannot be a base".to_string())?
            .pop_if_empty()
            .extend(["api", "v3"])
            .extend(segments);
        Ok(url)
    }

    fn request(&self, method: reqwest::Method, url: reqwest::Url) -> reqwest::RequestBuilder {
        let mut req = super::http_client()
            .request(method, url)
            .bearer_auth(&self.token);
        if let Some(id) = &self.deployment_id {
            req = req.header("Vqs-Deployment-Id", id);
        }
        req
    }

    /// SendMessage: put a job's envelope on `topic`, optionally delayed.
    pub(crate) async fn send(
        &self,
        topic: &str,
        envelope: &JobEnvelope,
        delay_secs: Option<u64>,
    ) -> Result<(), String> {
        let mut req = self
            .request(reqwest::Method::POST, self.url(&["topic", topic])?)
            .json(envelope);
        if let Some(secs) = delay_secs {
            req = req.header("Vqs-Delay-Seconds", secs.to_string());
        }
        let resp = req.send().await.map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(describe("send", resp).await)
        }
    }

    /// Acknowledge: end a delivery by its receipt handle. Idempotent.
    pub(crate) async fn acknowledge(&self, delivery: &Delivery) -> Result<(), String> {
        let url = self.url(&[
            "topic",
            &delivery.topic,
            "consumer",
            &delivery.consumer_group,
            "lease",
            &delivery.receipt_handle,
        ])?;
        let resp = self
            .request(reqwest::Method::DELETE, url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(describe("acknowledge", resp).await)
        }
    }
}

async fn describe(what: &str, resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let body: String = body.chars().take(300).collect();
    format!("queue {what} answered {status}: {body}")
}

/// Send a job's envelope to the jobs topic — [`super::deliver`] in queue mode.
pub(crate) async fn send(
    name: &str,
    id: &JobId,
    delay: Option<std::time::Duration>,
) -> Result<(), String> {
    let envelope = JobEnvelope::for_row(id, Some(name));
    let delay_secs = delay.and_then(|d| delay_seconds(d.as_millis() as i64));
    QueueClient::from_env(None)?
        .send(TOPIC, &envelope, delay_secs)
        .await
}

/// Send a rowless job: the whole job, first attempt, in the message.
pub(crate) async fn send_rowless(
    name: &str,
    id: &JobId,
    payload: serde_json::Value,
    max_attempts: u32,
) -> Result<(), String> {
    let envelope = JobEnvelope::rowless(id, name, payload, 1, max_attempts.max(1));
    QueueClient::from_env(None)?
        .send(TOPIC, &envelope, None)
        .await
}

// ------------------------------------------------------------------ consumer

/// What handling one delivery came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Handled {
    /// The attempt ran and succeeded.
    Succeeded,
    /// The attempt failed and a delayed message for the next one was sent.
    Retried,
    /// The attempt failed and its attempts are spent, or a stale run was
    /// reclaimed with none left.
    Dead,
    /// Nothing to run: the row is finished or gone.
    Dropped,
    /// Another delivery is running the row, or it could not be claimed.
    Left,
    /// The outcome is not safe yet (store or send failure): redeliver.
    Unsafe,
}

impl Handled {
    /// Whether the delivery is finished and must be acknowledged.
    pub(crate) fn acknowledge(self) -> bool {
        !matches!(self, Handled::Left | Handled::Unsafe)
    }
    fn as_str(self) -> &'static str {
        match self {
            Handled::Succeeded => "succeeded",
            Handled::Retried => "retried",
            Handled::Dead => "dead",
            Handled::Dropped => "dropped",
            Handled::Left => "left",
            Handled::Unsafe => "unsafe",
        }
    }
}

/// Handle one delivery for `entry`: decide, claim, run to completion, record,
/// send the retry. Everything except the acknowledgement, which the caller
/// does when [`Handled::acknowledge`] says so.
pub(crate) async fn process(
    store: &dyn JobStore,
    client: &QueueClient,
    entry: &crate::conventions::JobEntry,
    id: &JobId,
    ext: http::Extensions,
) -> Handled {
    macro_rules! store_try {
        ($what:literal, $call:expr) => {
            match $call {
                Ok(value) => value,
                Err(e) => {
                    tracing::error!(job = entry.name, id = %id, error = %e, concat!("queue delivery: ", $what, " failed"));
                    return Handled::Unsafe;
                }
            }
        };
    }

    let existing = store_try!("get", store.get(id).await);
    if let Some(row) = &existing {
        if row.name != entry.name {
            tracing::error!(job = entry.name, id = %id, row_job = %row.name,
                "queue delivery names a row of a different job; dropping it");
            return Handled::Dropped;
        }
    }
    match redelivery_decision(existing.as_ref(), now_ms(), entry.timeout_ms) {
        Redelivery::Drop => return Handled::Dropped,
        Redelivery::Leave => return Handled::Left,
        Redelivery::Run => {}
        Redelivery::Reclaim => {
            let cutoff = stale_cutoff(now_ms(), entry.timeout_ms);
            store_try!("reclaim", store.reclaim_one(id, cutoff).await);
            tracing::warn!(job = entry.name, id = %id, "reclaimed a stale run on redelivery");
        }
    }
    let Some(row) = store_try!("claim", store.claim(id).await) else {
        // Not claimable: a reclaim that spent the last attempt (now `dead`),
        // or another delivery got there first.
        return match store_try!("get", store.get(id).await) {
            Some(row) if row.status == JobStatus::Dead => Handled::Dead,
            Some(row) if row.status == JobStatus::Succeeded => Handled::Dropped,
            None => Handled::Dropped,
            Some(_) => Handled::Left,
        };
    };

    match super::run_attempt(Some(store), entry, row, ext).await {
        AttemptOutcome::Succeeded => Handled::Succeeded,
        AttemptOutcome::Dead => Handled::Dead,
        AttemptOutcome::Unrecorded => Handled::Unsafe,
        AttemptOutcome::Retry { delay_ms } => {
            let envelope = JobEnvelope::for_row(id, Some(entry.name));
            match client.send(TOPIC, &envelope, delay_seconds(delay_ms)).await {
                Ok(()) => Handled::Retried,
                Err(e) => {
                    // Leave this delivery unacknowledged: the queue brings it
                    // back, finds the row `failed`, and runs the retry then.
                    tracing::error!(job = entry.name, id = %id, error = %e,
                        "retry message was not sent; leaving the delivery for redelivery");
                    Handled::Unsafe
                }
            }
        }
    }
}

/// Rowless attempts running in this process, by `id#attempt`. With no row to
/// claim, this is the only double-run guard: a message redelivered to the
/// instance that is still running it is left alone. A redelivery to another
/// instance is not caught — keep rowless jobs shorter than the queue's
/// 5-minute visibility timeout, or safe to run twice.
static ROWLESS_RUNNING: std::sync::Mutex<Option<std::collections::HashSet<String>>> =
    std::sync::Mutex::new(None);

struct RowlessRun(String);

impl RowlessRun {
    fn begin(key: String) -> Option<Self> {
        let mut running = ROWLESS_RUNNING.lock().ok()?;
        // Build the guard only for a new key: a guard for a duplicate would
        // remove the first run's entry when it drops.
        let fresh = running
            .get_or_insert_with(Default::default)
            .insert(key.clone());
        drop(running);
        fresh.then(|| Self(key))
    }
}

impl Drop for RowlessRun {
    fn drop(&mut self) {
        if let Ok(mut running) = ROWLESS_RUNNING.lock() {
            if let Some(set) = running.as_mut() {
                set.remove(&self.0);
            }
        }
    }
}

/// Handle one delivery of a rowless job: the message carries the payload and
/// the attempt number, so run it, and on a retryable failure send the next
/// attempt as a new delayed message. Nothing is recorded anywhere but the
/// logs.
pub(crate) async fn process_rowless(
    client: &QueueClient,
    entry: &crate::conventions::JobEntry,
    envelope: JobEnvelope,
    ext: http::Extensions,
) -> Handled {
    let attempt = envelope.attempt.unwrap_or(1).max(1);
    let max_attempts = envelope.max_attempts.unwrap_or(entry.max_attempts).max(1);
    let Some(_running) = RowlessRun::begin(format!("{}#{attempt}", envelope.id)) else {
        return Handled::Left;
    };
    let now = now_ms();
    let payload = envelope.payload.unwrap_or(serde_json::Value::Null);
    let row = JobRow {
        id: envelope.id.clone(),
        name: entry.name.to_string(),
        payload: payload.clone(),
        status: JobStatus::Running,
        attempts: attempt,
        max_attempts,
        next_run_at: None,
        last_error: None,
        created_at: now,
        updated_at: now,
        result: None,
        history: Vec::new(),
    };
    match super::run_attempt(None, entry, row, ext).await {
        AttemptOutcome::Succeeded => Handled::Succeeded,
        AttemptOutcome::Dead => Handled::Dead,
        AttemptOutcome::Unrecorded => Handled::Unsafe,
        AttemptOutcome::Retry { delay_ms } => {
            let next =
                JobEnvelope::rowless(&envelope.id, entry.name, payload, attempt + 1, max_attempts);
            match client.send(TOPIC, &next, delay_seconds(delay_ms)).await {
                Ok(()) => Handled::Retried,
                Err(e) => {
                    // Not acknowledged: the queue delivers this attempt again.
                    tracing::error!(job = entry.name, id = %envelope.id, error = %e,
                        "retry message was not sent; leaving the delivery for redelivery");
                    Handled::Unsafe
                }
            }
        }
    }
}

/// The run route's queue branch: [`process`] with the process-wide store and
/// this deployment's client, then acknowledge.
pub(crate) async fn handle_delivery(
    entry: &crate::conventions::JobEntry,
    delivery: Delivery,
    req: http::Request<axum::body::Body>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    use http::StatusCode;

    let ext = req.extensions().clone();
    let client = match QueueClient::from_env(delivery.region.as_deref()) {
        Ok(client) => client,
        Err(e) => {
            tracing::error!(job = entry.name, error = %e, "queue delivery: no queue client");
            return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
        }
    };
    let body = axum::body::to_bytes(req.into_body(), MAX_MESSAGE_BYTES)
        .await
        .unwrap_or_default();
    let handled = match serde_json::from_slice::<JobEnvelope>(&body) {
        // A message that carries its payload is a rowless job, whatever
        // this process has for a store now.
        Ok(envelope) if envelope.payload.is_some() => {
            process_rowless(&client, entry, envelope, ext).await
        }
        Ok(envelope) => match super::store() {
            Ok(store) => process(store.as_ref(), &client, entry, &envelope.id, ext).await,
            Err(e) => {
                tracing::error!(job = entry.name, error = %e, "queue delivery: no job store");
                Handled::Unsafe
            }
        },
        Err(e) => {
            tracing::error!(job = entry.name, message_id = %delivery.message_id, error = %e,
                "queue delivery: unreadable envelope; dropping the message");
            Handled::Dropped
        }
    };

    let mut acknowledged = false;
    if handled.acknowledge() {
        match client.acknowledge(&delivery).await {
            Ok(()) => acknowledged = true,
            // Harmless: the redelivery finds the row finished and drops it.
            Err(e) => {
                tracing::warn!(job = entry.name, message_id = %delivery.message_id, error = %e,
                "queue acknowledge failed; the message will be delivered again")
            }
        }
    }
    tracing::info!(
        job = entry.name,
        message_id = %delivery.message_id,
        delivery = delivery.delivery_count,
        outcome = handled.as_str(),
        acknowledged,
        "queue delivery handled"
    );
    let status = if handled == Handled::Unsafe {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    let body = serde_json::json!({ "outcome": handled.as_str(), "acknowledged": acknowledged });
    let mut response = (status, axum::Json(body)).into_response();
    response.extensions_mut().insert(HandledMarker);
    response
}

/// Marks a response the queue branch of the run route produced, so the
/// consumer can tell "handled" from "the app's router answered something
/// else" (an unknown job, an auth layer).
#[derive(Clone, Copy)]
struct HandledMarker;

/// Wrap an app router as the queue consumer function.
///
/// `router` is the same router the main function serves (so jobs get the
/// app's `Extension<T>` state); the returned router answers queue deliveries
/// and nothing else. Calling it also marks this process as a deployment with
/// a consumer, so retries it sends go through the queue.
pub fn consumer(router: axum::Router) -> axum::Router {
    IS_CONSUMER.store(true, Ordering::Relaxed);
    let inner = Arc::new(router);
    axum::Router::new().fallback(move |req: axum::extract::Request| {
        let inner = Arc::clone(&inner);
        async move { consume(&inner, req).await }
    })
}

async fn consume(inner: &axum::Router, req: axum::extract::Request) -> axum::response::Response {
    use axum::response::IntoResponse;
    use http::StatusCode;
    use tower::util::ServiceExt;

    note_request(req.headers());
    let delivery = match parse_delivery(req.headers()) {
        Ok(delivery) => delivery,
        Err(reason) => return (StatusCode::NOT_FOUND, reason).into_response(),
    };
    let (mut parts, body) = req.into_parts();
    let bytes = match axum::body::to_bytes(body, MAX_MESSAGE_BYTES).await {
        Ok(bytes) => bytes,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    // The envelope's name picks the job's run route in the app router; the
    // delivery rides along as an extension, which only this process can set.
    let name = serde_json::from_slice::<JobEnvelope>(&bytes)
        .ok()
        .and_then(|envelope| envelope.name);
    let Some(uri) = name.as_deref().and_then(|name| {
        format!("{}/{}", super::NX_JOBS_PREFIX, name)
            .parse::<http::Uri>()
            .ok()
    }) else {
        tracing::error!(message_id = %delivery.message_id,
            "queue delivery has no job name; dropping the message");
        acknowledge_unroutable(&delivery).await;
        return StatusCode::OK.into_response();
    };
    parts.uri = uri;
    parts.method = http::Method::POST;
    parts.extensions.insert(delivery.clone());
    let response = match inner
        .clone()
        .oneshot(http::Request::from_parts(
            parts,
            axum::body::Body::from(bytes),
        ))
        .await
    {
        Ok(response) => response,
        Err(never) => match never {},
    };
    if response.extensions().get::<HandledMarker>().is_none() {
        // Not acknowledged: after a deploy that fixes the cause (a job that
        // is missing from this build, a layer in front of /__nx/jobs) the
        // redelivery runs it.
        tracing::error!(
            job = name.as_deref().unwrap_or(""),
            message_id = %delivery.message_id,
            status = response.status().as_u16(),
            "queue delivery did not reach the job's run route"
        );
    }
    prune_once().await;
    response
}

async fn acknowledge_unroutable(delivery: &Delivery) {
    match QueueClient::from_env(delivery.region.as_deref()) {
        Ok(client) => {
            if let Err(e) = client.acknowledge(delivery).await {
                tracing::warn!(message_id = %delivery.message_id, error = %e, "queue acknowledge failed");
            }
        }
        Err(e) => {
            tracing::warn!(message_id = %delivery.message_id, error = %e, "queue acknowledge skipped")
        }
    }
}

/// The sweep used to delete old finished rows. Without it, the consumer does,
/// once per instance.
async fn prune_once() {
    static PRUNED: AtomicBool = AtomicBool::new(false);
    // Nothing to prune without a store (rowless jobs).
    if PRUNED.swap(true, Ordering::Relaxed) || super::store().is_err() {
        return;
    }
    if let Err(e) = super::prune_expired().await {
        tracing::warn!(error = %e, "pruning old job rows failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::MemoryJobStore;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU32;

    fn headers(pairs: &[(&str, &str)]) -> http::HeaderMap {
        let mut map = http::HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                value.parse().unwrap(),
            );
        }
        map
    }

    /// The headers a Rust consumer received in the spike (values shortened).
    const SPIKE: &[(&str, &str)] = &[
        ("ce-type", "com.vercel.queue.v2beta"),
        ("ce-vqsmessageid", "msg_01"),
        ("ce-vqsqueuename", "nx-jobs"),
        ("ce-vqsconsumergroup", "api_Sjobs_Drs"),
        ("ce-vqsdeliverycount", "2"),
        ("ce-vqsreceipthandle", "rh/with+odd=chars"),
        ("ce-vqsregion", "pdx1"),
        ("user-agent", "vercel-queue/v2beta"),
    ];

    #[test]
    fn parses_a_delivery_from_ce_headers() {
        let delivery = parse_delivery(&headers(SPIKE)).unwrap();
        assert_eq!(
            delivery,
            Delivery {
                message_id: "msg_01".into(),
                topic: "nx-jobs".into(),
                consumer_group: "api_Sjobs_Drs".into(),
                receipt_handle: "rh/with+odd=chars".into(),
                delivery_count: 2,
                region: Some("pdx1".into()),
            }
        );
    }

    #[test]
    fn refuses_requests_that_are_not_deliveries() {
        assert!(
            parse_delivery(&http::HeaderMap::new())
                .unwrap_err()
                .contains("ce-type")
        );
        let v1 = headers(&[("ce-type", "com.vercel.queue.v1beta")]);
        assert!(parse_delivery(&v1).unwrap_err().contains("unsupported"));
        // Every header the acknowledgement needs is required.
        for missing in [
            "ce-vqsmessageid",
            "ce-vqsqueuename",
            "ce-vqsconsumergroup",
            "ce-vqsreceipthandle",
        ] {
            let rest: Vec<_> = SPIKE
                .iter()
                .copied()
                .filter(|(n, _)| *n != missing)
                .collect();
            assert_eq!(
                parse_delivery(&headers(&rest)).unwrap_err(),
                format!("missing {missing}")
            );
        }
        // The delivery count is informational; absent means first delivery.
        let rest: Vec<_> = SPIKE
            .iter()
            .copied()
            .filter(|(n, _)| *n != "ce-vqsdeliverycount")
            .collect();
        assert_eq!(parse_delivery(&headers(&rest)).unwrap().delivery_count, 1);
    }

    #[test]
    fn delay_is_whole_seconds_rounded_up_and_clamped() {
        assert_eq!(delay_seconds(0), None);
        assert_eq!(delay_seconds(-5), None);
        assert_eq!(delay_seconds(1), Some(1));
        assert_eq!(delay_seconds(2_000), Some(2));
        assert_eq!(delay_seconds(2_001), Some(3));
        assert_eq!(delay_seconds(3_600_000), Some(3_600));
        assert_eq!(delay_seconds(30 * 86_400_000), Some(7 * 86_400));
    }

    #[test]
    fn backoff_maps_to_the_queue_delay() {
        // audit-todo's policy: 2s doubling, capped at 30s.
        let delays: Vec<_> = (1..=6)
            .map(|attempt| delay_seconds(crate::jobs::backoff_ms(attempt, 2_000, 30_000)))
            .collect();
        assert_eq!(
            delays,
            [Some(2), Some(4), Some(8), Some(16), Some(30), Some(30)]
        );
    }

    fn row(id: &str, status: JobStatus, updated_at: i64) -> JobRow {
        JobRow {
            id: JobId(id.into()),
            name: "t".into(),
            payload: serde_json::json!({}),
            status,
            attempts: 1,
            max_attempts: 3,
            next_run_at: None,
            last_error: None,
            created_at: updated_at,
            updated_at,
            result: None,
            history: Vec::new(),
        }
    }

    #[test]
    fn redelivery_only_reclaims_a_run_past_its_own_timeout() {
        let now = 10_000_000;
        let timeout = 30_000;
        let decide = |status, age_ms| {
            redelivery_decision(Some(&row("a", status, now - age_ms)), now, timeout)
        };
        assert_eq!(redelivery_decision(None, now, timeout), Redelivery::Drop);
        assert_eq!(decide(JobStatus::Queued, 0), Redelivery::Run);
        assert_eq!(decide(JobStatus::Failed, 5_000), Redelivery::Run);
        assert_eq!(decide(JobStatus::Succeeded, 5_000), Redelivery::Drop);
        assert_eq!(decide(JobStatus::Dead, 5_000), Redelivery::Drop);
        // Running: fresh, at the edge, and past timeout + grace.
        assert_eq!(decide(JobStatus::Running, 1_000), Redelivery::Leave);
        assert_eq!(decide(JobStatus::Running, 90_000), Redelivery::Leave);
        assert_eq!(decide(JobStatus::Running, 90_001), Redelivery::Reclaim);
        // The 5-minute visibility redelivery of a 10-minute job leaves it be.
        assert_eq!(
            redelivery_decision(
                Some(&row("a", JobStatus::Running, now - 300_000)),
                now,
                600_000
            ),
            Redelivery::Leave
        );
    }

    // ------------------------------------------------- fake queue endpoint

    #[derive(Clone, Debug, PartialEq)]
    struct Seen {
        method: String,
        path: String,
        auth: Option<String>,
        deployment: Option<String>,
        delay: Option<String>,
        body: String,
    }

    #[derive(Clone, Default)]
    struct FakeQueue {
        seen: Arc<Mutex<Vec<Seen>>>,
        /// Status for SendMessage; the acknowledgement always answers 204.
        send_status: Arc<AtomicU32>,
    }

    impl FakeQueue {
        async fn start() -> (Self, QueueClient) {
            let fake = FakeQueue::default();
            fake.send_status.store(201, Ordering::Relaxed);
            let state = fake.clone();
            let app = axum::Router::new().fallback(move |req: axum::extract::Request| {
                let state = state.clone();
                async move {
                    let mut seen = {
                        let header = |name: &str| {
                            req.headers()
                                .get(name)
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_owned)
                        };
                        Seen {
                            method: req.method().to_string(),
                            path: req.uri().path().to_string(),
                            auth: header("authorization"),
                            deployment: header("vqs-deployment-id"),
                            delay: header("vqs-delay-seconds"),
                            body: String::new(),
                        }
                    };
                    let is_send = req.method() == http::Method::POST;
                    let bytes = axum::body::to_bytes(req.into_body(), 64 * 1024)
                        .await
                        .unwrap();
                    seen.body = String::from_utf8_lossy(&bytes).into_owned();
                    state.seen.lock().unwrap().push(seen);
                    let status = if is_send {
                        state.send_status.load(Ordering::Relaxed) as u16
                    } else {
                        204
                    };
                    http::StatusCode::from_u16(status).unwrap()
                }
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            (fake, QueueClient::new(&base, "tok", Some("dpl_1")))
        }
        fn seen(&self) -> Vec<Seen> {
            self.seen.lock().unwrap().clone()
        }
    }

    #[tokio::test]
    async fn send_posts_the_envelope_to_the_topic_with_the_delay() {
        let (fake, client) = FakeQueue::start().await;
        let envelope = JobEnvelope::for_row(&JobId("j1".into()), Some("audit-todo"));
        client.send(TOPIC, &envelope, None).await.unwrap();
        client.send(TOPIC, &envelope, Some(8)).await.unwrap();
        let seen = fake.seen();
        assert_eq!(seen[0].method, "POST");
        assert_eq!(seen[0].path, "/api/v3/topic/nx-jobs");
        assert_eq!(seen[0].auth.as_deref(), Some("Bearer tok"));
        assert_eq!(seen[0].deployment.as_deref(), Some("dpl_1"));
        assert_eq!(seen[0].delay, None);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&seen[0].body).unwrap(),
            serde_json::json!({ "id": "j1", "name": "audit-todo" })
        );
        assert_eq!(seen[1].delay.as_deref(), Some("8"));

        fake.send_status.store(403, Ordering::Relaxed);
        let err = client.send(TOPIC, &envelope, None).await.unwrap_err();
        assert!(err.contains("403"), "{err}");
    }

    #[tokio::test]
    async fn acknowledge_deletes_the_lease_by_receipt_handle() {
        let (fake, client) = FakeQueue::start().await;
        client
            .acknowledge(&parse_delivery(&headers(SPIKE)).unwrap())
            .await
            .unwrap();
        let seen = fake.seen();
        assert_eq!(seen[0].method, "DELETE");
        // The handle is one path segment, whatever characters it holds.
        assert_eq!(
            seen[0].path,
            "/api/v3/topic/nx-jobs/consumer/api_Sjobs_Drs/lease/rh%2Fwith+odd=chars"
        );
        assert_eq!(seen[0].auth.as_deref(), Some("Bearer tok"));
    }

    // ------------------------------------------------------------ process

    /// A job that fails while `fail_until` attempts have not passed.
    fn entry(fail_until: u32, max_attempts: u32) -> crate::conventions::JobEntry {
        crate::conventions::JobEntry {
            name: "t",
            run: crate::conventions::job_run_fn(move |_payload, _ext| async move {
                let attempt = crate::jobs::current().map_or(0, |job| job.attempt);
                if attempt <= fail_until {
                    Err(format!("boom {attempt}"))
                } else {
                    Ok(serde_json::json!({ "attempt": attempt }))
                }
            }),
            timeout_ms: 30_000,
            max_attempts,
            backoff_ms: 2_000,
            max_backoff_ms: 30_000,
        }
    }

    async fn insert(store: &MemoryJobStore, id: &str, max_attempts: u32) -> JobId {
        let mut r = row(id, JobStatus::Queued, now_ms());
        r.attempts = 0;
        r.max_attempts = max_attempts;
        store.insert(r).await.unwrap();
        JobId(id.into())
    }

    #[tokio::test]
    async fn a_delivery_runs_the_job_to_completion() {
        let (fake, client) = FakeQueue::start().await;
        let store = MemoryJobStore::new();
        let id = insert(&store, "ok", 3).await;
        let handled = process(&store, &client, &entry(0, 3), &id, http::Extensions::new()).await;
        assert_eq!(handled, Handled::Succeeded);
        assert!(handled.acknowledge());
        let got = store.get(&id).await.unwrap().unwrap();
        assert_eq!(got.status, JobStatus::Succeeded);
        assert_eq!(got.result, Some(serde_json::json!({ "attempt": 1 })));
        assert!(fake.seen().is_empty(), "no retry message for a success");
        // The same message delivered again finds the row finished.
        let again = process(&store, &client, &entry(0, 3), &id, http::Extensions::new()).await;
        assert_eq!(again, Handled::Dropped);
        assert!(again.acknowledge());
    }

    #[tokio::test]
    async fn a_failed_attempt_sends_a_delayed_message_then_succeeds() {
        let (fake, client) = FakeQueue::start().await;
        let store = MemoryJobStore::new();
        let id = insert(&store, "flaky", 3).await;
        let job = entry(2, 3); // fails attempts 1 and 2

        for (attempt, delay) in [(1, "2"), (2, "4")] {
            let handled = process(&store, &client, &job, &id, http::Extensions::new()).await;
            assert_eq!(handled, Handled::Retried);
            let got = store.get(&id).await.unwrap().unwrap();
            assert_eq!((got.status, got.attempts), (JobStatus::Failed, attempt));
            let sent = fake.seen().pop().unwrap();
            assert_eq!(sent.path, "/api/v3/topic/nx-jobs");
            assert_eq!(sent.delay.as_deref(), Some(delay));
        }
        let handled = process(&store, &client, &job, &id, http::Extensions::new()).await;
        assert_eq!(handled, Handled::Succeeded);
        let got = store.get(&id).await.unwrap().unwrap();
        assert_eq!(
            got.history.iter().map(|a| a.n).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(fake.seen().len(), 2, "one retry message per failed attempt");
    }

    #[tokio::test]
    async fn spent_attempts_end_dead_without_another_message() {
        let (fake, client) = FakeQueue::start().await;
        let store = MemoryJobStore::new();
        let id = insert(&store, "dead", 1).await;
        let handled = process(&store, &client, &entry(9, 1), &id, http::Extensions::new()).await;
        assert_eq!(handled, Handled::Dead);
        assert!(handled.acknowledge());
        assert_eq!(
            store.get(&id).await.unwrap().unwrap().status,
            JobStatus::Dead
        );
        assert!(fake.seen().is_empty());
    }

    #[tokio::test]
    async fn a_retry_that_cannot_be_sent_is_left_for_redelivery() {
        let (fake, client) = FakeQueue::start().await;
        fake.send_status.store(500, Ordering::Relaxed);
        let store = MemoryJobStore::new();
        let id = insert(&store, "nosend", 3).await;
        let handled = process(&store, &client, &entry(1, 3), &id, http::Extensions::new()).await;
        assert_eq!(handled, Handled::Unsafe);
        assert!(!handled.acknowledge());
        // The redelivery finds the row `failed` and runs the next attempt.
        let again = process(&store, &client, &entry(1, 3), &id, http::Extensions::new()).await;
        assert_eq!(again, Handled::Succeeded);
    }

    /// Put a row into `running`, last touched `age_ms` ago — an instance that
    /// claimed it and then died (or is still going).
    async fn running(
        store: &MemoryJobStore,
        id: &str,
        attempts: u32,
        max: u32,
        age_ms: i64,
    ) -> JobId {
        let mut r = row(id, JobStatus::Running, now_ms() - age_ms);
        r.attempts = attempts;
        r.max_attempts = max;
        store.insert(r).await.unwrap();
        JobId(id.into())
    }

    #[tokio::test]
    async fn redelivery_reclaims_a_stale_run_and_runs_it_again() {
        let (_fake, client) = FakeQueue::start().await;
        let store = MemoryJobStore::new();
        // Claimed five minutes ago by an instance that died; timeout is 30s.
        let id = running(&store, "crashed", 1, 3, 300_000).await;
        let handled = process(&store, &client, &entry(0, 3), &id, http::Extensions::new()).await;
        assert_eq!(handled, Handled::Succeeded);
        let got = store.get(&id).await.unwrap().unwrap();
        assert_eq!((got.status, got.attempts), (JobStatus::Succeeded, 2));
    }

    #[tokio::test]
    async fn redelivery_of_a_stale_run_with_no_attempts_left_ends_dead() {
        let (_fake, client) = FakeQueue::start().await;
        let store = MemoryJobStore::new();
        let id = running(&store, "killer", 3, 3, 300_000).await;
        let handled = process(&store, &client, &entry(0, 3), &id, http::Extensions::new()).await;
        assert_eq!(handled, Handled::Dead);
        assert!(handled.acknowledge());
        let got = store.get(&id).await.unwrap().unwrap();
        assert_eq!((got.status, got.attempts), (JobStatus::Dead, 3));
    }

    #[tokio::test]
    async fn redelivery_leaves_a_live_run_alone() {
        let (_fake, client) = FakeQueue::start().await;
        let store = MemoryJobStore::new();
        let id = running(&store, "live", 1, 3, 1_000).await;
        let handled = process(&store, &client, &entry(0, 3), &id, http::Extensions::new()).await;
        assert_eq!(handled, Handled::Left);
        assert!(
            !handled.acknowledge(),
            "the message is the live run's crash insurance"
        );
        let got = store.get(&id).await.unwrap().unwrap();
        assert_eq!((got.status, got.attempts), (JobStatus::Running, 1));
    }

    #[tokio::test]
    async fn reclaiming_one_row_leaves_other_stale_rows_alone() {
        let store = MemoryJobStore::new();
        let a = running(&store, "a", 1, 3, 300_000).await;
        let b = running(&store, "b", 1, 3, 300_000).await;
        assert!(store.reclaim_one(&a, now_ms() - 90_000).await.unwrap());
        assert_eq!(
            store.get(&a).await.unwrap().unwrap().status,
            JobStatus::Failed
        );
        assert_eq!(
            store.get(&b).await.unwrap().unwrap().status,
            JobStatus::Running
        );
        // Not stale by this cutoff, and not running any more: nothing to do.
        assert!(!store.reclaim_one(&b, now_ms() - 600_000).await.unwrap());
        assert!(!store.reclaim_one(&a, now_ms()).await.unwrap());
    }

    #[tokio::test]
    async fn a_row_of_another_job_is_dropped_not_run() {
        let (_fake, client) = FakeQueue::start().await;
        let store = MemoryJobStore::new();
        let mut r = row("other", JobStatus::Queued, now_ms());
        r.name = "someone-else".into();
        store.insert(r).await.unwrap();
        let id = JobId("other".into());
        let handled = process(&store, &client, &entry(0, 3), &id, http::Extensions::new()).await;
        assert_eq!(handled, Handled::Dropped);
        assert_eq!(
            store.get(&id).await.unwrap().unwrap().status,
            JobStatus::Queued
        );
    }

    #[tokio::test]
    async fn a_rowless_job_runs_from_its_message_alone() {
        let (fake, client) = FakeQueue::start().await;
        let job = entry(1, 3); // fails attempt 1
        let first = JobEnvelope::rowless(
            &JobId("r1".into()),
            "t",
            serde_json::json!({"k": "v"}),
            1,
            3,
        );
        let handled = process_rowless(&client, &job, first, http::Extensions::new()).await;
        assert_eq!(handled, Handled::Retried);
        assert!(handled.acknowledge());
        // The retry is a new message for attempt 2, with the payload again.
        let sent = fake.seen().pop().unwrap();
        assert_eq!(sent.delay.as_deref(), Some("2"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&sent.body).unwrap(),
            serde_json::json!({
                "id": "r1", "name": "t", "payload": {"k": "v"}, "attempt": 2, "max_attempts": 3
            })
        );
        let second: JobEnvelope = serde_json::from_str(&sent.body).unwrap();
        let handled = process_rowless(&client, &job, second, http::Extensions::new()).await;
        assert_eq!(handled, Handled::Succeeded);
        assert_eq!(fake.seen().len(), 1, "a success sends nothing more");
    }

    #[tokio::test]
    async fn a_rowless_job_stops_at_its_last_attempt() {
        let (fake, client) = FakeQueue::start().await;
        let last = JobEnvelope::rowless(&JobId("r2".into()), "t", serde_json::json!({}), 3, 3);
        let handled = process_rowless(&client, &entry(9, 3), last, http::Extensions::new()).await;
        assert_eq!(handled, Handled::Dead);
        assert!(handled.acknowledge());
        assert!(fake.seen().is_empty());
    }

    #[tokio::test]
    async fn a_rowless_retry_that_cannot_be_sent_is_left_for_redelivery() {
        let (fake, client) = FakeQueue::start().await;
        fake.send_status.store(500, Ordering::Relaxed);
        let first = JobEnvelope::rowless(&JobId("r3".into()), "t", serde_json::json!({}), 1, 3);
        let handled = process_rowless(&client, &entry(1, 3), first, http::Extensions::new()).await;
        assert_eq!(handled, Handled::Unsafe);
        assert!(!handled.acknowledge(), "acknowledging would lose the job");
    }

    #[tokio::test]
    async fn a_rowless_attempt_already_running_here_is_left_alone() {
        let (_fake, client) = FakeQueue::start().await;
        let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
        let (finish_tx, finish_rx) = tokio::sync::oneshot::channel::<()>();
        let started_tx = Arc::new(Mutex::new(Some(started_tx)));
        let finish_rx = Arc::new(tokio::sync::Mutex::new(Some(finish_rx)));
        let slow = crate::conventions::JobEntry {
            name: "t",
            run: crate::conventions::job_run_fn(move |_payload, _ext| {
                let started_tx = Arc::clone(&started_tx);
                let finish_rx = Arc::clone(&finish_rx);
                async move {
                    if let Some(tx) = started_tx.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    if let Some(rx) = finish_rx.lock().await.take() {
                        let _ = rx.await;
                    }
                    Ok(serde_json::Value::Null)
                }
            }),
            timeout_ms: 30_000,
            max_attempts: 3,
            backoff_ms: 2_000,
            max_backoff_ms: 30_000,
        };
        let message =
            || JobEnvelope::rowless(&JobId("r4".into()), "t", serde_json::json!({}), 1, 3);
        let first = process_rowless(&client, &slow, message(), http::Extensions::new());
        let redelivery = async {
            started_rx.await.unwrap();
            // The same message again while the first run is still going.
            let handled = process_rowless(&client, &slow, message(), http::Extensions::new()).await;
            finish_tx.send(()).unwrap();
            handled
        };
        let (first, redelivery) = tokio::join!(first, redelivery);
        assert_eq!(first, Handled::Succeeded);
        assert_eq!(redelivery, Handled::Left);
        assert!(!redelivery.acknowledge());
    }

    #[tokio::test]
    async fn consumer_answers_only_queue_deliveries() {
        use tower::util::ServiceExt;
        let app = consumer(axum::Router::new());
        let response = app
            .oneshot(
                http::Request::post("/api/jobs")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), http::StatusCode::NOT_FOUND);
    }
}
