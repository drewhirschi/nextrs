pub mod conventions;
pub mod discovery;
pub mod openapi;
pub mod router;

pub use axum;
pub use http;
pub use utoipa;

/// `#[nextrs::api(...)]` — typed API handler with the OpenAPI path derived from
/// the file location. See [`nextrs_macros::api`].
pub use nextrs_macros::api;

#[cfg(feature = "vercel")]
pub mod vercel;

#[cfg(feature = "build")]
pub mod build;
