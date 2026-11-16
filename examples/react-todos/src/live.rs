//! Realtime for the todo list (nextrs::realtime): every mutation tells open
//! lists "todos changed" and each tab refetches through the typed API. The
//! frame carries the change (upsert/delete + key) for clients that want to
//! patch their cache instead; app/page.tsx simply invalidates.

use nextrs::realtime::RealtimeChange;

/// The one topic: every viewer watches the same list.
pub const TODOS_TOPIC: &str = "todos";

/// Publish after the response (fan-out never delays the mutation). Locally
/// this is the in-process hub; on Vercel the Cloudflare relay
/// (NEXTRS_REALTIME_URL).
pub fn todos_changed(wait: &nextrs::WaitUntil, change: RealtimeChange<nextrs::serde_json::Value>) {
    wait.wait_until(async move {
        if let Err(e) = nextrs::realtime::publish(TODOS_TOPIC, change).await {
            tracing::warn!(error = %e, "realtime publish failed");
        }
    });
}
