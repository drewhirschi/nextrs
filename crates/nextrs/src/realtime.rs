//! Realtime: push "this changed" to every browser watching a topic.
//!
//! The pattern: a mutation commits, then [`publish`]es an ordered change to a
//! topic (`"todos"`, `"household.42"`); subscribed clients apply it or refetch
//! their source-of-truth query. Your Rust app keeps every decision — who may
//! subscribe ([`ticket`]), what a change carries, where data comes from. The
//! transport only fans frames out.
//!
//! Two transports, one wire format ([`RealtimeFrame`]):
//!
//! - **In-process hub** (default): WebSockets served by this process at
//!   `/__nx/realtime/{topic}`. Right for `nextrs dev`, a single long-lived
//!   server, or a container — every subscriber is in the process that
//!   publishes.
//! - **Cloudflare relay** (`NEXTRS_REALTIME_URL`): serverless instances can't
//!   share sockets, so production on Vercel publishes over HTTP to a small,
//!   generated Durable Object worker (`nextrs realtime generate`) that holds
//!   the sockets — hibernated, so idle connections cost ~nothing — and keeps
//!   the per-topic sequence. The worker has no app logic and is never edited.
//!
//! Auth is a signed ticket, not cookies, so the relay can live on another
//! domain: your route decides, then returns [`ticket`]`(topic)` — a URL
//! carrying `{exp}.{HMAC-SHA256(NEXTRS_REALTIME_SECRET, topic + "\n" + exp)}`.
//! Publishing to the relay sends the same secret as a bearer token.
//!
//! Clients recover from either transport the same way: fetch the snapshot
//! after `ready`, apply `change` frames in sequence order, refetch on a gap or
//! a `resync`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;

/// Where the in-process hub (and the generated relay) serve topics.
pub const REALTIME_PREFIX: &str = "/__nx/realtime";
const TICKET_TTL_SECS: i64 = 300;
const CHANNEL_CAPACITY: usize = 256;
const KEEPALIVE: Duration = Duration::from_secs(25);

// ------------------------------------------------------------------ protocol

/// A database-shaped change a client can apply to a collection directly.
/// `invalidate` is the escape hatch: refetch the authoritative query.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RealtimeChange<T = Value> {
    pub operation: RealtimeOperation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<T>,
}

impl<T> RealtimeChange<T> {
    pub fn upsert(key: impl Into<String>, value: T) -> Self {
        Self { operation: RealtimeOperation::Upsert, key: Some(key.into()), value: Some(value) }
    }
    pub fn delete(key: impl Into<String>) -> Self {
        Self { operation: RealtimeOperation::Delete, key: Some(key.into()), value: None }
    }
    pub fn invalidate() -> Self {
        Self { operation: RealtimeOperation::Invalidate, key: None, value: None }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RealtimeOperation {
    Upsert,
    Delete,
    Invalidate,
}

/// Frames every transport sends.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RealtimeFrame<T = Value> {
    /// Attached. Reconcile with the snapshot now (closes the fetch/subscribe race).
    Ready { topic: String, sequence: u64 },
    /// One ordered change within a topic.
    Change {
        topic: String,
        sequence: u64,
        #[serde(flatten)]
        change: RealtimeChange<T>,
    },
    /// Changes were dropped; refetch before continuing.
    Resync { topic: String, sequence: u64 },
}

/// Topic names: 1–128 of `A-Z a-z 0-9 . _ : -`.
pub fn valid_topic(topic: &str) -> bool {
    !topic.is_empty()
        && topic.len() <= 128
        && topic.bytes().all(|b| b.is_ascii_alphanumeric() || b".:_-".contains(&b))
}

// ------------------------------------------------------------- configuration

fn on_vercel() -> bool {
    std::env::var_os("VERCEL").is_some()
}

/// The relay base URL (`NEXTRS_REALTIME_URL`), when production uses one.
pub fn relay_url() -> Option<String> {
    std::env::var("NEXTRS_REALTIME_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|v| v.trim_end_matches('/').to_string())
}

static DEV_SECRET: OnceLock<String> = OnceLock::new();

/// `NEXTRS_REALTIME_SECRET`; off Vercel without a relay, a random
/// per-process secret (tickets are only checked by this process then).
fn secret() -> Result<String, RealtimeError> {
    if let Ok(s) = std::env::var("NEXTRS_REALTIME_SECRET") {
        if s.len() >= 16 {
            return Ok(s);
        }
        return Err(RealtimeError::Config("NEXTRS_REALTIME_SECRET must be at least 16 characters".into()));
    }
    if on_vercel() || relay_url().is_some() {
        return Err(RealtimeError::Config(
            "set NEXTRS_REALTIME_SECRET (shared with the relay worker) — tickets and publishes are signed with it".into(),
        ));
    }
    Ok(DEV_SECRET
        .get_or_init(|| format!("dev-{:016x}{:016x}", crate::logs::random_u64(), crate::logs::random_u64()))
        .clone())
}

/// Why a realtime call failed.
#[derive(Debug)]
pub enum RealtimeError {
    Config(String),
    InvalidTopic(String),
    Encode(serde_json::Error),
    Relay(String),
}

impl std::fmt::Display for RealtimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RealtimeError::Config(m) => write!(f, "nextrs realtime: {m}"),
            RealtimeError::InvalidTopic(t) => write!(f, "nextrs realtime: invalid topic `{t}`"),
            RealtimeError::Encode(e) => write!(f, "nextrs realtime: change did not serialize: {e}"),
            RealtimeError::Relay(m) => write!(f, "nextrs realtime: relay: {m}"),
        }
    }
}
impl std::error::Error for RealtimeError {}

// ------------------------------------------------------------------- tickets

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn mac(secret: &str, topic: &str, exp: i64) -> String {
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).expect("hmac takes any key");
    mac.update(format!("{topic}\n{exp}").as_bytes());
    hex(&mac.finalize().into_bytes())
}

fn sign(secret: &str, topic: &str, now_secs: i64) -> String {
    let exp = now_secs + TICKET_TTL_SECS;
    format!("{exp}.{}", mac(secret, topic, exp))
}

fn verify(secret: &str, topic: &str, token: &str, now_secs: i64) -> bool {
    let Some((exp, sig)) = token.split_once('.') else { return false };
    let Ok(exp) = exp.parse::<i64>() else { return false };
    let want = mac(secret, topic, exp);
    exp > now_secs
        && sig.len() == want.len()
        && sig.bytes().zip(want.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

/// What a client needs to subscribe: a URL with a signed, short-lived token.
/// `url` is absolute for the relay, origin-relative for the in-process hub.
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Ticket {
    pub topic: String,
    pub url: String,
    /// Unix seconds; fetch a new ticket to reconnect after this.
    pub expires_at: i64,
}

/// Sign a subscription to `topic`. Call it from your own route, *after*
/// deciding the current user may watch that topic.
pub fn ticket(topic: &str) -> Result<Ticket, RealtimeError> {
    if !valid_topic(topic) {
        return Err(RealtimeError::InvalidTopic(topic.into()));
    }
    let secret = secret()?;
    let now = crate::logs::now_ms() / 1000;
    let token = sign(&secret, topic, now);
    let path = format!("{REALTIME_PREFIX}/{topic}?token={token}");
    let url = match relay_url() {
        Some(base) => format!("{base}{path}"),
        None => path,
    };
    Ok(Ticket { topic: topic.into(), url, expires_at: now + TICKET_TTL_SECS })
}

// ------------------------------------------------------------------- publish

/// Broadcast `change` to `topic`'s subscribers; returns its sequence number.
///
/// Call it after the write commits — typically inside `wait.wait_until(...)`
/// so the response doesn't wait on the fan-out. With a relay configured this
/// is one HTTP POST; otherwise it's an in-process broadcast.
pub async fn publish<T: Serialize>(topic: &str, change: RealtimeChange<T>) -> Result<u64, RealtimeError> {
    if !valid_topic(topic) {
        return Err(RealtimeError::InvalidTopic(topic.into()));
    }
    let Some(base) = relay_url() else {
        if on_vercel() {
            return Err(RealtimeError::Config(
                "no relay on Vercel — instances don't share sockets; set NEXTRS_REALTIME_URL to the \
                 relay worker (`nextrs realtime generate`) and NEXTRS_REALTIME_SECRET"
                    .into(),
            ));
        }
        return hub().publish(topic, change).map_err(RealtimeError::Encode);
    };
    let secret = secret()?;
    let resp = http_client()
        .post(format!("{base}{REALTIME_PREFIX}/{topic}"))
        .bearer_auth(secret)
        .json(&change)
        .send()
        .await
        .map_err(|e| RealtimeError::Relay(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(RealtimeError::Relay(format!("answered {}", resp.status())));
    }
    #[derive(Deserialize)]
    struct Published {
        sequence: u64,
    }
    resp.json::<Published>()
        .await
        .map(|p| p.sequence)
        .map_err(|e| RealtimeError::Relay(e.to_string()))
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("nextrs realtime: could not build the HTTP client")
    })
}

// ------------------------------------------------------------ in-process hub

struct Topic {
    sequence: u64,
    sender: broadcast::Sender<String>,
}

/// The single-process broker: one broadcast channel per topic. No history —
/// a lagging client gets `resync` and refetches.
#[derive(Default)]
struct Hub {
    topics: Mutex<HashMap<String, Topic>>,
}

fn hub() -> &'static Hub {
    static HUB: OnceLock<Hub> = OnceLock::new();
    HUB.get_or_init(Hub::default)
}

impl Hub {
    fn with_topic<R>(&self, topic: &str, f: impl FnOnce(&mut Topic) -> R) -> R {
        let mut topics = self.topics.lock().unwrap_or_else(|e| e.into_inner());
        let entry = topics.entry(topic.to_owned()).or_insert_with(|| Topic {
            sequence: 0,
            sender: broadcast::channel(CHANNEL_CAPACITY).0,
        });
        f(entry)
    }

    fn publish<T: Serialize>(&self, topic: &str, change: RealtimeChange<T>) -> Result<u64, serde_json::Error> {
        self.with_topic(topic, |t| {
            t.sequence += 1;
            let frame = serde_json::to_string(&RealtimeFrame::Change {
                topic: topic.to_owned(),
                sequence: t.sequence,
                change,
            })?;
            // No subscribers is normal: the next one reconciles from a snapshot.
            let _ = t.sender.send(frame);
            Ok(t.sequence)
        })
    }

    fn subscribe(&self, topic: &str) -> (u64, broadcast::Receiver<String>) {
        self.with_topic(topic, |t| (t.sequence, t.sender.subscribe()))
    }
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

/// `GET /__nx/realtime/{topic}?token=…` — the in-process hub's WebSocket.
/// 404 when a relay is configured (this process wouldn't see other
/// instances' publishes).
pub(crate) fn router() -> Router {
    Router::new().route(&format!("{REALTIME_PREFIX}/{{topic}}"), get(connect))
}

async fn connect(Path(topic): Path<String>, Query(q): Query<TokenQuery>, upgrade: WebSocketUpgrade) -> Response {
    if relay_url().is_some() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !valid_topic(&topic) {
        return (StatusCode::BAD_REQUEST, "invalid topic").into_response();
    }
    let Ok(secret) = secret() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let ok = q
        .token
        .as_deref()
        .is_some_and(|t| verify(&secret, &topic, t, crate::logs::now_ms() / 1000));
    if !ok {
        return (StatusCode::FORBIDDEN, "missing or expired ticket").into_response();
    }
    upgrade.on_upgrade(move |socket| serve(socket, topic))
}

async fn serve(mut socket: WebSocket, topic: String) {
    let (sequence, mut rx) = hub().subscribe(&topic);
    let send = |frame: &RealtimeFrame| serde_json::to_string(frame).ok();
    let Some(ready) = send(&RealtimeFrame::Ready { topic: topic.clone(), sequence }) else { return };
    if socket.send(Message::Text(ready.into())).await.is_err() {
        return;
    }
    let mut keepalive = tokio::time::interval(KEEPALIVE);
    keepalive.tick().await;
    loop {
        tokio::select! {
            received = rx.recv() => match received {
                Ok(frame) => {
                    if socket.send(Message::Text(frame.into())).await.is_err() {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let (sequence, fresh) = hub().subscribe(&topic);
                    rx = fresh;
                    let Some(frame) = send(&RealtimeFrame::Resync { topic: topic.clone(), sequence }) else { return };
                    if socket.send(Message::Text(frame.into())).await.is_err() {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            incoming = socket.recv() => match incoming {
                // Receive-only: ignore client messages, stop when it leaves.
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                Some(Ok(_)) => {}
            },
            _ = keepalive.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return;
                }
            }
        }
    }
}

pub use crate::realtime_relay::WORKER_JS as RELAY_WORKER_JS;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tickets_verify_expire_and_bind_to_their_topic() {
        let now = 1_800_000_000;
        let t = sign("0123456789abcdef", "todos", now);
        assert!(verify("0123456789abcdef", "todos", &t, now));
        assert!(!verify("0123456789abcdef", "todos.other", &t, now), "bound to its topic");
        assert!(!verify("another-secret-xx", "todos", &t, now), "bound to the secret");
        assert!(!verify("0123456789abcdef", "todos", &t, now + TICKET_TTL_SECS + 1), "expires");
        let (_, sig) = t.split_once('.').unwrap();
        let forged = format!("{}.{sig}", now + 10 * TICKET_TTL_SECS);
        assert!(!verify("0123456789abcdef", "todos", &forged, now), "expiry is signed");
    }

    #[test]
    fn hmac_matches_the_worker_vector() {
        // Independent vector (Python hmac); relay-worker.js is checked against
        // the same one, so the two signers can't drift.
        assert_eq!(
            mac("0123456789abcdef", "todos", 1_800_000_300),
            "977635521679a6b3dfe6f01fe74bcf81898c0fe67b8939ebbd9cf4671b1561ea"
        );
    }

    #[test]
    fn topic_names() {
        assert!(valid_topic("todos"));
        assert!(valid_topic("household.42:photos"));
        assert!(!valid_topic(""));
        assert!(!valid_topic("a/b"));
        assert!(!valid_topic(&"x".repeat(129)));
    }

    #[tokio::test]
    async fn hub_publishes_ordered_frames() {
        let hub = Hub::default();
        let (start, mut rx) = hub.subscribe("t");
        assert_eq!(start, 0);
        assert_eq!(hub.publish("t", RealtimeChange::upsert("1", json!({"id": 1}))).unwrap(), 1);
        assert_eq!(hub.publish("t", RealtimeChange::<Value>::delete("1")).unwrap(), 2);
        let first: RealtimeFrame = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        assert_eq!(
            first,
            RealtimeFrame::Change { topic: "t".into(), sequence: 1, change: RealtimeChange::upsert("1", json!({"id": 1})) }
        );
        let wire: Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        assert_eq!(wire, json!({"type": "change", "topic": "t", "sequence": 2, "operation": "delete", "key": "1"}));
    }
}
