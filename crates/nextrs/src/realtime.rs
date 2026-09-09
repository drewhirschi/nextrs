//! Realtime change protocol and a single-process WebSocket broker.
//!
//! [`MemoryRealtime`] is the local-development adapter for the protocol. It is
//! deliberately not a distributed broker: production serverless deployments
//! should put the same `RealtimeFrame` wire format behind a coordinator such
//! as a Cloudflare Durable Object. Clients recover from either adapter in the
//! same way: fetch a source-of-truth snapshot after `ready`, then apply ordered
//! `change` frames until the connection asks them to `resync`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::get;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;

const DEFAULT_CHANNEL_CAPACITY: usize = 256;
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(25);

/// A database-shaped change that can update a client collection directly.
///
/// `invalidate` is the escape hatch for a mutation whose affected rows cannot
/// be represented cheaply. The client should refetch its authoritative query
/// when it sees that operation.
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
        Self {
            operation: RealtimeOperation::Upsert,
            key: Some(key.into()),
            value: Some(value),
        }
    }

    pub fn delete(key: impl Into<String>) -> Self {
        Self {
            operation: RealtimeOperation::Delete,
            key: Some(key.into()),
            value: None,
        }
    }

    pub fn invalidate() -> Self {
        Self {
            operation: RealtimeOperation::Invalidate,
            key: None,
            value: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RealtimeOperation {
    Upsert,
    Delete,
    Invalidate,
}

/// Frames sent by every nextrs realtime transport.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RealtimeFrame<T = Value> {
    /// The subscription is attached. Clients should now reconcile with the
    /// source-of-truth snapshot to close the fetch/subscribe race.
    Ready { topic: String, sequence: u64 },
    /// One ordered change within a topic.
    Change {
        topic: String,
        sequence: u64,
        #[serde(flatten)]
        change: RealtimeChange<T>,
    },
    /// The transport dropped one or more changes; refetch before continuing.
    Resync { topic: String, sequence: u64 },
}

#[derive(Clone)]
struct Topic {
    sequence: u64,
    sender: broadcast::Sender<String>,
}

struct Subscription {
    sequence: u64,
    receiver: broadcast::Receiver<String>,
}

/// A keyed, single-process broker for local development and tests.
///
/// Clone it into application state, merge [`MemoryRealtime::router`] into the
/// app, and publish after a database transaction commits. It intentionally
/// retains no event history; a lagged client receives `resync` and refetches.
#[derive(Clone)]
pub struct MemoryRealtime {
    topics: Arc<Mutex<HashMap<String, Topic>>>,
    channel_capacity: usize,
}

impl Default for MemoryRealtime {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryRealtime {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CHANNEL_CAPACITY)
    }

    pub fn with_capacity(channel_capacity: usize) -> Self {
        assert!(
            channel_capacity > 0,
            "realtime channel capacity must be positive"
        );
        Self {
            topics: Arc::new(Mutex::new(HashMap::new())),
            channel_capacity,
        }
    }

    /// Serve browser subscriptions at `/__nx/realtime/{topic}`.
    pub fn router(self) -> Router {
        Router::new()
            .route("/__nx/realtime/{topic}", get(connect))
            .with_state(self)
    }

    /// Broadcast a change to the topic and return its monotonically increasing
    /// in-process sequence number.
    pub fn publish<T: Serialize>(
        &self,
        topic: impl AsRef<str>,
        change: RealtimeChange<T>,
    ) -> Result<u64, serde_json::Error> {
        let topic = topic.as_ref();
        let mut topics = self.topics.lock().expect("realtime topic lock poisoned");
        let entry = topics.entry(topic.to_owned()).or_insert_with(|| {
            let (sender, _) = broadcast::channel(self.channel_capacity);
            Topic {
                sequence: 0,
                sender,
            }
        });
        entry.sequence += 1;
        let sequence = entry.sequence;
        let frame = RealtimeFrame::Change {
            topic: topic.to_owned(),
            sequence,
            change,
        };
        let encoded = serde_json::to_string(&frame)?;
        // No receivers is a normal state: changes are advisory and the next
        // subscriber reconciles from the authoritative snapshot.
        let _ = entry.sender.send(encoded);
        Ok(sequence)
    }

    fn subscribe(&self, topic: &str) -> Subscription {
        let mut topics = self.topics.lock().expect("realtime topic lock poisoned");
        let entry = topics.entry(topic.to_owned()).or_insert_with(|| {
            let (sender, _) = broadcast::channel(self.channel_capacity);
            Topic {
                sequence: 0,
                sender,
            }
        });
        Subscription {
            sequence: entry.sequence,
            receiver: entry.sender.subscribe(),
        }
    }
}

async fn connect(
    State(realtime): State<MemoryRealtime>,
    Path(topic): Path<String>,
    upgrade: WebSocketUpgrade,
) -> Response {
    upgrade.on_upgrade(move |socket| serve_socket(socket, realtime, topic))
}

async fn serve_socket(mut socket: WebSocket, realtime: MemoryRealtime, topic: String) {
    let mut subscription = realtime.subscribe(&topic);
    let ready = RealtimeFrame::<Value>::Ready {
        topic: topic.clone(),
        sequence: subscription.sequence,
    };
    let Ok(ready) = serde_json::to_string(&ready) else {
        return;
    };
    if socket.send(Message::Text(ready.into())).await.is_err() {
        return;
    }

    let mut keepalive = tokio::time::interval(KEEPALIVE_INTERVAL);
    keepalive.tick().await;
    loop {
        tokio::select! {
            received = subscription.receiver.recv() => match received {
                Ok(frame) => {
                    if socket.send(Message::Text(frame.into())).await.is_err() {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let sequence = realtime.subscribe(&topic).sequence;
                    let frame = RealtimeFrame::<Value>::Resync {
                        topic: topic.clone(),
                        sequence,
                    };
                    let Ok(frame) = serde_json::to_string(&frame) else {
                        return;
                    };
                    if socket.send(Message::Text(frame.into())).await.is_err() {
                        return;
                    }
                    subscription = realtime.subscribe(&topic);
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            _ = keepalive.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn publishes_ordered_database_shaped_changes() {
        let realtime = MemoryRealtime::new();
        let mut subscription = realtime.subscribe("household.demo");

        assert_eq!(
            realtime
                .publish(
                    "household.demo",
                    RealtimeChange::upsert("photo-1", json!({ "id": "photo-1" })),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            realtime
                .publish("household.demo", RealtimeChange::<Value>::delete("photo-1"),)
                .unwrap(),
            2
        );

        let first: RealtimeFrame =
            serde_json::from_str(&subscription.receiver.recv().await.unwrap()).unwrap();
        let second: RealtimeFrame =
            serde_json::from_str(&subscription.receiver.recv().await.unwrap()).unwrap();
        assert_eq!(
            first,
            RealtimeFrame::Change {
                topic: "household.demo".to_owned(),
                sequence: 1,
                change: RealtimeChange::upsert("photo-1", json!({ "id": "photo-1" })),
            }
        );
        assert_eq!(
            second,
            RealtimeFrame::Change {
                topic: "household.demo".to_owned(),
                sequence: 2,
                change: RealtimeChange::delete("photo-1"),
            }
        );
    }

    #[test]
    fn publishing_without_subscribers_still_advances_the_cursor() {
        let realtime = MemoryRealtime::new();
        realtime
            .publish(
                "household.demo",
                RealtimeChange::upsert("1", json!({ "id": 1 })),
            )
            .unwrap();

        assert_eq!(realtime.subscribe("household.demo").sequence, 1);
    }
}
