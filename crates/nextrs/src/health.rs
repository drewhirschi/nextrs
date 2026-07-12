//! Unified start-temperature telemetry: `GET /__nx/health`.
//!
//! Every nextrs app answers this endpoint the same way, so one external
//! pinger can measure cold vs. warm starts uniformly across the whole fleet
//! (see the framework repo's `.github/workflows/coldstart-pinger.yml`).
//!
//! The response does no I/O — it reports raw process facts and lets the
//! caller judge temperature:
//!
//! - `boot_id`: random per process. A changed `boot_id` between pings means a
//!   new instance served the second ping.
//! - `uptime_ms`: ms since the router was built. On serverless, a request
//!   that *caused* the instance to start sees a tiny uptime.
//! - `first_request`: true exactly once per process — the strongest
//!   "this request paid the cold start" signal.
//!
//! Header mirrors (`x-nextrs-boot-id`, `x-nextrs-uptime-ms`, `x-nextrs-cold`)
//! carry the same facts for callers that don't parse bodies.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Reserved path of the health/telemetry endpoint.
pub const NX_HEALTH_PATH: &str = "/__nx/health";

static BOOT: OnceLock<(Instant, u64)> = OnceLock::new();
static SERVED: AtomicBool = AtomicBool::new(false);

/// Record process boot. Called from router construction; first call wins, so
/// `uptime_ms` measures from the first router built in this process.
pub(crate) fn init() {
    BOOT.get_or_init(|| (Instant::now(), boot_id()));
}

/// A random-enough per-process id without adding an RNG dependency: the
/// process id folded with the wall-clock nanos at boot.
fn boot_id() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    // SplitMix64 finalizer — decorrelates the low-entropy inputs.
    let mut z = nanos ^ (pid << 32) ^ pid;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

pub(crate) async fn handler() -> axum::response::Response {
    let (start, id) = *BOOT.get_or_init(|| (Instant::now(), boot_id()));
    let uptime_ms = start.elapsed().as_millis() as u64;
    let first = !SERVED.swap(true, Ordering::Relaxed);

    let body = format!(
        "{{\"status\":\"ok\",\"boot_id\":\"{id:016x}\",\"uptime_ms\":{uptime_ms},\"first_request\":{first}}}"
    );
    axum::response::Response::builder()
        .status(http::StatusCode::OK)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .header("x-nextrs-boot-id", format!("{id:016x}"))
        .header("x-nextrs-uptime-ms", uptime_ms.to_string())
        .header("x-nextrs-cold", if first { "1" } else { "0" })
        .body(axum::body::Body::from(body))
        .expect("static health response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_reports_ok_and_cold_only_once() {
        init();
        let first = handler().await;
        assert_eq!(first.status(), http::StatusCode::OK);
        let cold = first.headers().get("x-nextrs-cold").unwrap();
        // Another test body may have hit the process-global flag first; the
        // invariant is that AFTER any hit, subsequent responses are warm.
        let _ = cold;
        let second = handler().await;
        assert_eq!(second.headers().get("x-nextrs-cold").unwrap(), "0");
        let body = axum::body::to_bytes(second.into_body(), 1 << 16)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("\"status\":\"ok\""));
        assert!(text.contains("\"boot_id\":\""));
        assert!(text.contains("\"first_request\":false"));
    }
}
