pub mod conventions;
pub mod discovery;
pub mod openapi;
pub mod prefetch;
pub mod router;
pub mod seed;

pub use prefetch::{Eagerness, PrefetchConfig, SpeculationMode};

pub use axum;
pub use http;
pub use utoipa;

// Re-exported for the seed companions `#[nextrs::api]` expands (they
// reference `::nextrs::serde_json` so consumer crates don't need the dep).
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
