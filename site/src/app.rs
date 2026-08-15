//! Shared documentation-site application construction.
//!
//! `src/main.rs` and `api/index.rs` are process adapters. Routes, OpenAPI, and
//! application-wide router configuration live here so local and Vercel
//! execution cannot drift apart.

include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

/// Build the application shared by the local server and deployment adapters.
pub fn app() -> axum::Router {
    // public/ sits next to app/. Locally the framework serves it as a fallback;
    // on Vercel the CDN handles it before the catch-all function rewrite.
    let public_dir = std::env::var("NEXTRS_PUBLIC_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/public").to_string());

    // The docs routes are server-rendered, so opt them into browser-native
    // link prefetch. React app-shell routes are automatically excluded.
    nextrs::router::build_router_with_public_and_speculation(
        generated_registry(),
        &public_dir,
        nextrs::SpeculationConfig {
            mode: nextrs::SpeculationMode::Prefetch,
            eagerness: nextrs::Eagerness::Moderate,
        },
    )
    .merge(nextrs::openapi::spec_router(generated_openapi()))
}

/// Bind the requested local port, falling forward to the next available one.
pub async fn bind_with_fallback(start: u16) -> tokio::net::TcpListener {
    for port in start..start.saturating_add(20) {
        match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
            Ok(listener) => {
                if port != start {
                    tracing::warn!(
                        "Port {start} is in use; bound {port} instead (set PORT to choose)."
                    );
                }
                return listener;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(error) => {
                eprintln!("Failed to bind 0.0.0.0:{port}: {error}");
                std::process::exit(1);
            }
        }
    }
    eprintln!(
        "No free port in {start}..{}. Stop the process using it, or set PORT.",
        start.saturating_add(20)
    );
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::app;
    use axum::body::{Body, to_bytes};
    use http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn shared_app_keeps_speculation_enabled_for_docs_pages() {
        let response = app()
            .oneshot(Request::builder().uri("/docs").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();

        assert!(html.contains(r#"<script type="speculationrules">"#));
    }
}
