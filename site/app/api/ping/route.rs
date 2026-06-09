//! Example of a *typed* `route.rs`.
//!
//! Handlers use Axum extractors with concrete return types, and each is
//! annotated with `#[utoipa::path]`. nextrs's codegen collects these into the
//! app's OpenAPI document (`generated_openapi()`), which is served at
//! `/openapi.json` and drives the generated TypeScript / React Query client
//! under `site/client/`.
//!
//! The `path = "..."` in each annotation must match this file's route
//! (`app/api/ping` → `/api/ping`).

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
    // operation_id and tag aren't required, but they're worth setting: the
    // client generator derives hook names from operation_id (→ `useGetPing`)
    // and groups endpoints by tag. Without them you'd get bare `useGet` and a
    // tag named after the internal codegen module.
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
    request_body = PingRequest,
    responses((status = 200, description = "Echoes the posted message", body = PingResponse)),
)]
pub async fn post(Json(req): Json<PingRequest>) -> Json<PingResponse> {
    Json(PingResponse {
        message: req.message,
        pong: true,
    })
}
