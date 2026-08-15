//! Shared React Todos application construction.
//!
//! `src/main.rs` and `api/index.rs` are process adapters. Application state,
//! routes, and middleware belong here so local and Vercel execution cannot
//! drift apart.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

// Convention modules generated from app/ refer to the domain crate by its
// package name. Make that name available while those modules compile as part
// of this library root.
extern crate self as react_todos;

pub mod core;

include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

/// Build the application shared by the local server and deployment adapters.
pub fn app() -> axum::Router {
    // Locally, Axum serves public/. On Vercel the CDN handles those files
    // before its catch-all rewrite; a missing runtime directory is a no-op.
    let public_dir = std::env::var("NEXTRS_PUBLIC_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/public").to_string());

    // App state as an Extension layer — handlers extract it, and seed
    // companions read it from request extensions during prefetch.
    nextrs::router::build_router_with_public(generated_registry(), &public_dir)
        .merge(nextrs::openapi::spec_router(generated_openapi()))
        .layer(axum::Extension(core::todos::TodosCtx::new()))
}

/// Add the benchmark headers used by the React Todos Vercel deployment.
///
/// This is kept out of [`app`] so local requests retain their existing shape,
/// while the Vercel adapter remains too small to accumulate application logic.
pub fn with_cold_start_headers(app: axum::Router) -> axum::Router {
    // Vercel exposes no cold/warm signal, so report one. BOOT is captured once
    // per router construction; FIRST_SEEN flips on the process's first request.
    static FIRST_SEEN: AtomicBool = AtomicBool::new(false);
    let boot = Instant::now();
    let instance_id: axum::http::HeaderValue = {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        format!("{:x}-{:x}", std::process::id(), nanos)
            .parse()
            .expect("process instance ID is a valid header value")
    };

    app.layer(axum::middleware::map_response(
        move |mut response: axum::response::Response| {
            let instance_id = instance_id.clone();
            async move {
                let cold = !FIRST_SEEN.swap(true, Ordering::Relaxed);
                let headers = response.headers_mut();
                headers.insert(
                    "x-cold",
                    if cold { "1" } else { "0" }
                        .parse()
                        .expect("cold marker is a valid header value"),
                );
                if let Ok(value) = boot.elapsed().as_millis().to_string().parse() {
                    headers.insert("x-init-ms", value);
                }
                headers.insert("x-instance", instance_id);
                response
            }
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::{app, with_cold_start_headers};
    use axum::body::Body;
    use http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn vercel_benchmark_middleware_keeps_all_cold_start_headers() {
        let response = with_cold_start_headers(app())
            .oneshot(
                Request::builder()
                    .uri(nextrs::health::NX_HEALTH_PATH)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(response.headers().contains_key("x-cold"));
        assert!(response.headers().contains_key("x-init-ms"));
        assert!(response.headers().contains_key("x-instance"));
    }
}
