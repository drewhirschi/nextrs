pub mod conventions;
pub mod health;
pub mod discovery;
pub mod openapi;
pub mod params;
pub mod router;
pub mod seed;
pub mod speculation;
pub mod telemetry;
pub mod wait_until;

/// Deprecated path for [`speculation`] — kept for one release. This module
/// only ever controlled document-level Speculation Rules; the data-prefetch
/// convention (`prefetch.rs`, `/__nx/prefetch`) lives elsewhere and the old
/// name conflated the two.
#[deprecated(note = "renamed to `nextrs::speculation` — this only controls document-level Speculation Rules, not data prefetch")]
pub mod prefetch {
    #[allow(deprecated)]
    pub use crate::speculation::*;
}

pub use speculation::{Eagerness, SpeculationConfig, SpeculationMode};
#[allow(deprecated)]
pub use speculation::PrefetchConfig;

pub use axum;
pub use http;
pub use utoipa;

// Re-exported for the seed companions `#[nextrs::api]` expands (they
// reference `::nextrs::serde_json` so consumer crates don't need the dep).
pub use params::{Params, search_params};
pub use telemetry::Timing;
pub use wait_until::WaitUntil;
pub use seed::{QuerySeed, SeedEntry, seed_key};
pub use serde_json;

/// `#[nextrs::api(...)]` — typed API handler with the OpenAPI path derived from
/// the file location. See [`nextrs_macros::api`].
pub use nextrs_macros::api;

#[cfg(feature = "vercel")]
pub mod vercel;

#[cfg(feature = "build")]
pub mod build;

#[cfg(feature = "build")]
pub mod docs;

#[cfg(feature = "tsx")]
pub mod bundle;
