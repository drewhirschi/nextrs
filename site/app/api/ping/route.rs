//! Example of a *typed* `route.rs`.
//!
//! Handlers use Axum extractors with concrete return types, and each is
//! annotated with `#[utoipa::path]`. nextrs's codegen collects these into the
//! app's OpenAPI document (`generated_openapi()`), which is served at
//! `/openapi.json` and drives the generated TypeScript / React Query client
//! under `site/client/`.
//!
//! What the annotation needs (with the `axum_extras` feature on):
//!   - `method` + `path` — required by utoipa. `path` must match this file's
//!     route (`app/api/ping` → `/api/ping`).
//!   - `responses(...)` — required to get a *typed response*; utoipa does not
//!     infer it from the `Json<T>` return type.
//!   - the request body is **inferred** from the `Json<PingRequest>` extractor,
//!     so there's no `request_body = ...` to write.
//!   - `operation_id` / `tag` are optional polish: they give the generated hook
//!     a clean name (`useGetPing`) and group it. Without them you'd get
//!     `useGet` and a tag named after the internal codegen module.

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

#[utoipa::path(
    get,
    path = "/api/ping",
    operation_id = "getPing",
    tag = "ping",
    responses((status = 200, description = "Pong", body = PingResponse)),
)]
pub async fn get() -> Json<PingResponse> {
    Json(PingResponse {
        message: "pong".to_string(),
        pong: true,
    })
}

#[utoipa::path(
    post,
    path = "/api/ping",
    operation_id = "sendPing",
    tag = "ping",
    responses((status = 200, description = "Echoes the posted message", body = PingResponse)),
)]
pub async fn post(Json(req): Json<PingRequest>) -> Json<PingResponse> {
    Json(PingResponse {
        message: req.message,
        pong: true,
    })
}
