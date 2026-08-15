//! [`ApiError`] — the framework's error convention for API routes.
//!
//! The recommended handler shape for anything fallible is
//! `Result<Json<T>, ApiError>`: the `Ok` side stays the inferred success body,
//! and the `Err` side carries a status code plus a typed JSON body
//! (`{ "error": "...", "code": "..." }`) instead of an opaque `StatusCode`.
//!
//! `#[nextrs::api]` recognizes the shape structurally: a handler returning
//! `Result<Json<T>, ApiError>` gets a `default` error response with the
//! [`ApiError`] schema injected into its OpenAPI operation automatically, so
//! the generated client sees a typed error union without any hand-written
//! `responses(...)` block.
//!
//! ```ignore
//! use nextrs::ApiError;
//!
//! #[nextrs::api(get)]
//! pub async fn get(Path(id): Path<u64>) -> Result<Json<TodoDetail>, ApiError> {
//!     let todo = ctx.get(id).await
//!         .ok_or_else(|| ApiError::not_found("no todo with that id"))?;
//!     Ok(Json(todo.into()))
//! }
//! ```

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A status code plus a typed JSON error body.
///
/// Serializes as `{ "error": "...", "code": "..." }` (`code` omitted when
/// absent); the status travels in the response line, not the body. Construct
/// via the status shorthands ([`ApiError::not_found`], [`ApiError::bad_request`],
/// …) or [`ApiError::new`] for anything else, and chain [`ApiError::with_code`]
/// to add a machine-readable code clients can match on.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiError {
    /// HTTP status for the response line. Not part of the wire body.
    #[serde(skip, default = "default_status")]
    #[schema(ignore)]
    pub status: StatusCode,
    /// Human-readable description of what went wrong.
    pub error: String,
    /// Optional machine-readable code (`"todo_not_found"`) for clients that
    /// need to branch on the failure without string-matching `error`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

fn default_status() -> StatusCode {
    StatusCode::INTERNAL_SERVER_ERROR
}

impl ApiError {
    /// An error with an arbitrary status.
    pub fn new(status: StatusCode, error: impl Into<String>) -> Self {
        Self {
            status,
            error: error.into(),
            code: None,
        }
    }

    /// 400 Bad Request.
    pub fn bad_request(error: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, error)
    }

    /// 401 Unauthorized.
    pub fn unauthorized(error: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, error)
    }

    /// 403 Forbidden.
    pub fn forbidden(error: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, error)
    }

    /// 404 Not Found.
    pub fn not_found(error: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, error)
    }

    /// 409 Conflict.
    pub fn conflict(error: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, error)
    }

    /// 422 Unprocessable Entity.
    pub fn unprocessable(error: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, error)
    }

    /// 500 Internal Server Error.
    pub fn internal(error: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, error)
    }

    /// Attach a machine-readable code to the body.
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self)).into_response()
    }
}

/// `StatusCode` handlers migrate with a `?`-friendly `From`: the body's
/// `error` is the status's canonical reason (`"Not Found"`).
impl From<StatusCode> for ApiError {
    fn from(status: StatusCode) -> Self {
        Self::new(status, status.canonical_reason().unwrap_or("error"))
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.status, self.error)
    }
}

impl std::error::Error for ApiError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_omits_status_and_absent_code() {
        let e = ApiError::not_found("no todo with that id");
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"error":"no todo with that id"}"#
        );
    }

    #[test]
    fn body_includes_code_when_set() {
        let e = ApiError::not_found("no todo").with_code("todo_not_found");
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"error":"no todo","code":"todo_not_found"}"#
        );
    }

    #[test]
    fn response_carries_status_and_json_content_type() {
        let resp = ApiError::conflict("already exists").into_response();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/json"
        );
    }

    #[test]
    fn from_status_uses_canonical_reason() {
        let e: ApiError = StatusCode::NOT_FOUND.into();
        assert_eq!(e.status, StatusCode::NOT_FOUND);
        assert_eq!(e.error, "Not Found");
    }
}
