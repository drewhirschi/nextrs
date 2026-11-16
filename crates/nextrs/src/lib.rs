pub mod conventions;
pub mod cron;
pub mod discovery;
pub mod error;
pub mod health;
pub mod logs;
pub mod openapi;
pub mod params;
pub mod router;
pub mod rsx;
pub mod seed;
pub mod speculation;
pub mod telemetry;
pub mod wait_until;
#[cfg(feature = "admin")]
pub mod admin;
#[cfg(feature = "jobs")]
pub mod jobs;
#[cfg(feature = "libsql")]
mod db;

/// A 256-bit hex secret from the OS RNG, for per-process fallback secrets.
/// Never derived from time or pids: a fallback can end up guarding a real
/// single-server deployment.
#[cfg_attr(not(any(feature = "jobs", feature = "realtime")), allow(dead_code))]
pub(crate) fn os_random_hex() -> String {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("nextrs: the OS random number generator is unavailable");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
#[cfg(feature = "realtime")]
pub mod realtime;

/// The Cloudflare relay worker for [`realtime`](crate::realtime), as source.
/// Always compiled (no feature) so the CLI can generate it without pulling in
/// the runtime's WebSocket and HTTP-client dependencies.
pub mod realtime_relay {
    /// `worker.js` for `nextrs realtime generate`. Its ticket HMAC and frame
    /// format match `nextrs::realtime` (shared test vector in both).
    pub const WORKER_JS: &str = include_str!("realtime/relay-worker.js");
}

/// Deprecated path for [`speculation`] — kept for one release. This module
/// only ever controlled document-level Speculation Rules; the data-prefetch
/// convention (`prefetch.rs`, `/__nx/prefetch`) lives elsewhere and the old
/// name conflated the two.
#[deprecated(
    note = "renamed to `nextrs::speculation` — this only controls document-level Speculation Rules, not data prefetch"
)]
pub mod prefetch {
    #[allow(deprecated)]
    pub use crate::speculation::*;
}

#[allow(deprecated)]
pub use speculation::PrefetchConfig;
pub use speculation::{Eagerness, SpeculationConfig, SpeculationMode};

pub use axum;
pub use http;
pub use utoipa;

// Re-exported for the seed companions `#[nextrs::api]` expands (they
// reference `::nextrs::serde_json` so consumer crates don't need the dep).
pub use error::ApiError;
pub use rsx::Rsx;
pub use params::{Params, search_params};
pub use seed::{QuerySeed, SeedEntry, seed_key};
pub use serde_json;
// Re-exported for generated island bindings (`#[serde(crate = "::nextrs::serde")]`),
// so consumer crates don't need a direct serde dependency for the derive.
pub use serde;
pub use telemetry::Timing;
pub use wait_until::WaitUntil;

/// `#[nextrs::api(...)]` — typed API handler with the OpenAPI path derived from
/// the file location. See [`nextrs_macros::api`].
pub use nextrs_macros::api;
/// `#[nextrs::cron(schedule = "...")]` — a scheduled `#[nextrs::api]` plus the `CRON_SECRET` bearer gate
/// for scheduled routes. See [`nextrs_macros::cron`] and [`cron::authorize`].
pub use nextrs_macros::cron;
/// `rsx! { <main>...</main> }` — JSX-shaped HTML for Rust server components,
/// returning [`rsx::Rsx`]. See [`nextrs_macros::rsx`] and `docs/rsx-server-components.md`.
pub use nextrs_macros::rsx;
/// `#[nextrs::job]` — a background job in `app/jobs/<name>/job.rs` with retries and back-off.
/// See [`nextrs_macros::job`] and [`jobs`] (feature `jobs`).
pub use nextrs_macros::job;

#[cfg(feature = "vercel")]
pub mod vercel;

#[cfg(feature = "build")]
pub mod build;

#[cfg(feature = "build")]
pub mod docs;

#[cfg(feature = "tsx")]
pub mod bundle;

#[cfg(feature = "tsx")]
pub mod islands;

#[cfg(feature = "server-bundles")]
pub mod server_bundles;

/// Framework version compiled into this application.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
