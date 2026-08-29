//! Cron endpoint authentication.
//!
//! Cron-triggered routes are ordinary API routes that must reject callers
//! other than the configured trigger (a generated Cloudflare Worker or a
//! native Vercel cron). Both send `Authorization: Bearer $CRON_SECRET`.
//!
//! The check is fail-closed: if `CRON_SECRET` is unset in the environment,
//! every request is rejected rather than letting the route run open.

use axum::extract::FromRequestParts;
use axum::response::{IntoResponse, Response};
use http::request::Parts;
use http::{HeaderMap, StatusCode};
use serde::Serialize;

/// Request-parts extractor injected by `#[nextrs::cron]`.
///
/// Because it is the first handler argument, authentication rejects the
/// request before Axum reads or parses a body-consuming extractor.
pub struct CronAuth;

impl<S> FromRequestParts<S> for CronAuth
where
    S: Send + Sync,
{
    type Rejection = CronAuthRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        authorize_detailed(&parts.headers)?;
        Ok(Self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CronAuthRejection {
    SecretNotConfigured,
    AuthorizationMissing,
    AuthorizationMalformed,
    SecretMismatch,
}

#[derive(Serialize)]
struct CronAuthErrorBody {
    error: &'static str,
    reason: &'static str,
    message: &'static str,
}

impl IntoResponse for CronAuthRejection {
    fn into_response(self) -> Response {
        let (status, reason, message) = match self {
            Self::SecretNotConfigured => (
                StatusCode::SERVICE_UNAVAILABLE,
                "secret_not_configured",
                "CRON_SECRET is not configured on this deployment",
            ),
            Self::AuthorizationMissing => (
                StatusCode::UNAUTHORIZED,
                "authorization_missing",
                "Authorization header is required",
            ),
            Self::AuthorizationMalformed => (
                StatusCode::UNAUTHORIZED,
                "authorization_malformed",
                "Authorization must use the Bearer scheme",
            ),
            Self::SecretMismatch => (
                StatusCode::UNAUTHORIZED,
                "secret_mismatch",
                "Bearer token does not match CRON_SECRET",
            ),
        };
        let mut response = (
            status,
            axum::Json(CronAuthErrorBody {
                error: if status == StatusCode::SERVICE_UNAVAILABLE {
                    "cron_not_configured"
                } else {
                    "cron_unauthorized"
                },
                reason,
                message,
            }),
        )
            .into_response();
        if status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                http::header::WWW_AUTHENTICATE,
                http::HeaderValue::from_static("Bearer"),
            );
        }
        response
    }
}

/// Authorize a cron request against the `CRON_SECRET` environment variable.
///
/// Returns `Ok(())` when the request carries `Authorization: Bearer <secret>`
/// matching `CRON_SECRET`. Returns `401` for missing/malformed credentials and
/// `503` when the deployment itself has no configured secret (fail-closed).
///
/// ```ignore
/// pub async fn get(headers: axum::http::HeaderMap) -> Result<Json<Value>, StatusCode> {
///     nextrs::cron::authorize(&headers)?;
///     // ... the actual work ...
/// }
/// ```
pub fn authorize(headers: &HeaderMap) -> Result<(), StatusCode> {
    authorize_detailed(headers).map_err(|rejection| match rejection {
        CronAuthRejection::SecretNotConfigured => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::UNAUTHORIZED,
    })
}

fn authorize_detailed(headers: &HeaderMap) -> Result<(), CronAuthRejection> {
    let Ok(secret) = std::env::var("CRON_SECRET") else {
        tracing::warn!("cron request rejected: CRON_SECRET is not set (fail-closed)");
        return Err(CronAuthRejection::SecretNotConfigured);
    };
    if secret.is_empty() {
        tracing::warn!("cron request rejected: CRON_SECRET is empty (fail-closed)");
        return Err(CronAuthRejection::SecretNotConfigured);
    }
    let Some(value) = headers.get(http::header::AUTHORIZATION) else {
        return Err(CronAuthRejection::AuthorizationMissing);
    };
    let Ok(value) = value.to_str() else {
        return Err(CronAuthRejection::AuthorizationMalformed);
    };
    let Some(token) = value
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty())
    else {
        return Err(CronAuthRejection::AuthorizationMalformed);
    };
    if constant_time_eq(token.as_bytes(), secret.as_bytes()) {
        Ok(())
    } else {
        Err(CronAuthRejection::SecretMismatch)
    }
}

/// Length-safe constant-time comparison; avoids leaking the secret through
/// early-exit timing on a hot public endpoint.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::header::AUTHORIZATION;

    fn headers(value: Option<&str>) -> HeaderMap {
        let mut map = HeaderMap::new();
        if let Some(value) = value {
            map.insert(AUTHORIZATION, value.parse().unwrap());
        }
        map
    }

    // Env-var tests run in one #[test] because std::env is process-global.
    #[tokio::test]
    async fn authorize_is_fail_closed_exact_and_precedes_body_extraction() {
        unsafe { std::env::remove_var("CRON_SECRET") };
        assert_eq!(
            authorize_detailed(&headers(Some("Bearer anything"))),
            Err(CronAuthRejection::SecretNotConfigured)
        );

        unsafe { std::env::set_var("CRON_SECRET", "s3cret") };
        assert_eq!(
            authorize_detailed(&headers(None)),
            Err(CronAuthRejection::AuthorizationMissing)
        );
        assert_eq!(
            authorize_detailed(&headers(Some("Bearer wrong"))),
            Err(CronAuthRejection::SecretMismatch)
        );
        assert_eq!(
            authorize_detailed(&headers(Some("s3cret"))),
            Err(CronAuthRejection::AuthorizationMalformed)
        );
        assert!(authorize(&headers(Some("Bearer s3cret"))).is_ok());

        async fn protected(
            _: CronAuth,
            axum::Json(_): axum::Json<serde_json::Value>,
        ) -> StatusCode {
            StatusCode::NO_CONTENT
        }
        use tower::ServiceExt as _;
        let app = axum::Router::new().route("/", axum::routing::post(protected));
        let unauthorized = app
            .clone()
            .oneshot(
                http::Request::post("/")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from("{"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(unauthorized.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("authorization_missing"));

        let authorized_bad_json = app
            .oneshot(
                http::Request::post("/")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .header(http::header::AUTHORIZATION, "Bearer s3cret")
                    .body(axum::body::Body::from("{"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(authorized_bad_json.status(), StatusCode::BAD_REQUEST);
        unsafe { std::env::remove_var("CRON_SECRET") };
    }
}
