//! Cron endpoint authentication.
//!
//! Cron-triggered routes are ordinary API routes that must reject callers
//! other than the configured trigger (a generated Cloudflare Worker or a
//! native Vercel cron). Both send `Authorization: Bearer $CRON_SECRET`.
//!
//! The check is fail-closed: if `CRON_SECRET` is unset in the environment,
//! every request is rejected rather than letting the route run open.

use http::{HeaderMap, StatusCode};

/// Authorize a cron request against the `CRON_SECRET` environment variable.
///
/// Returns `Ok(())` when the request carries `Authorization: Bearer <secret>`
/// matching `CRON_SECRET`. Returns `Err(StatusCode::UNAUTHORIZED)` on any
/// mismatch, missing header, or missing secret (fail-closed).
///
/// ```ignore
/// pub async fn get(headers: axum::http::HeaderMap) -> Result<Json<Value>, StatusCode> {
///     nextrs::cron::authorize(&headers)?;
///     // ... the actual work ...
/// }
/// ```
pub fn authorize(headers: &HeaderMap) -> Result<(), StatusCode> {
    let Ok(secret) = std::env::var("CRON_SECRET") else {
        tracing::warn!("cron request rejected: CRON_SECRET is not set (fail-closed)");
        return Err(StatusCode::UNAUTHORIZED);
    };
    if secret.is_empty() {
        tracing::warn!("cron request rejected: CRON_SECRET is empty (fail-closed)");
        return Err(StatusCode::UNAUTHORIZED);
    }
    let presented = headers
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    match presented {
        Some(token) if constant_time_eq(token.as_bytes(), secret.as_bytes()) => Ok(()),
        _ => Err(StatusCode::UNAUTHORIZED),
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
    #[test]
    fn authorize_is_fail_closed_and_exact() {
        unsafe { std::env::remove_var("CRON_SECRET") };
        assert!(authorize(&headers(Some("Bearer anything"))).is_err());

        unsafe { std::env::set_var("CRON_SECRET", "s3cret") };
        assert!(authorize(&headers(None)).is_err());
        assert!(authorize(&headers(Some("Bearer wrong"))).is_err());
        assert!(authorize(&headers(Some("s3cret"))).is_err());
        assert!(authorize(&headers(Some("Bearer s3cret"))).is_ok());
        unsafe { std::env::remove_var("CRON_SECRET") };
    }
}
