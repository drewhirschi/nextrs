//! Example of a *typed* `route.rs`.
//!
//! Handlers use Axum extractors with concrete return types, and each is
//! annotated with `#[nextrs::api]`. nextrs's codegen collects these into the
//! app's OpenAPI document (`generated_openapi()`), which is served at
//! `/openapi.json` and drives the generated TypeScript / React Query client
//! under `site/client/`.
//!
//! `#[nextrs::api]` is a thin wrapper over `#[utoipa::path]` that **derives the
//! `path` from this file's location** (`app/api/ping/route.rs` → `/api/ping`),
//! so the URL isn't restated. What's left to write:
//!   - the method (`get` / `post`) — first argument.
//!   - `responses(...)` — required for a *typed response*; it isn't inferred
//!     from the `Json<T>` return type.
//!   - the request body is **inferred** from the `Json<PingRequest>` extractor.
//!   - `operation_id` / `tag` are derived from the route (here `getApiPing`,
//!     tag `ping`) unless you set them — `post` overrides `operation_id` below
//!     to get a nicer `useSendPing()` hook.
//!
//! You can still use `#[utoipa::path(...)]` directly for full control; the
//! codegen then checks its `path` against this file's URL.

use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Response returned by both `GET` and `POST /api/ping`.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct PingResponse {
    /// Echoed back from the request (or a default for `GET`).
    pub message: String,
    /// Always `true` — proves the handler ran.
    pub pong: bool,
}

/// Request body for `POST /api/ping`.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct PingRequest {
    /// Message to echo back in the response.
    pub message: String,
}

#[nextrs::api(
    get,
    responses((status = 200, description = "Pong", body = PingResponse)),
)]
pub async fn get() -> Json<PingResponse> {
    Json(PingResponse {
        message: "pong".to_string(),
        pong: true,
    })
}

#[nextrs::api(
    post,
    operation_id = "sendPing",
    responses((status = 200, description = "Echoes the posted message", body = PingResponse)),
)]
pub async fn post(Json(req): Json<PingRequest>) -> Json<PingResponse> {
    Json(PingResponse {
        message: req.message,
        pong: true,
    })
}
