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
//! so the URL isn't restated. The method comes from the function name, request
//! and response bodies come from `Json<T>`, and `operation_id` / `tag` come
//! from the route. `post` below overrides only its public client name to get a
//! nicer `useSendPing()` hook.
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

#[nextrs::api]
pub async fn get() -> Json<PingResponse> {
    Json(PingResponse {
        message: "pong".to_string(),
        pong: true,
    })
}

#[nextrs::api(operation_id = "sendPing")]
pub async fn post(Json(req): Json<PingRequest>) -> Json<PingResponse> {
    Json(PingResponse {
        message: req.message,
        pong: true,
    })
}
