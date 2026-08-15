//! Local/container process entry point.
//!
//! The application itself is constructed in `src/app.rs`; keep this file
//! limited to local environment, diagnostics, and serving.

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    if let Ok(path) = std::env::var("NEXTRS_ENV_FILE") {
        dotenvy::from_path(path).ok();
    } else {
        dotenvy::dotenv().ok();
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let app = site::app();

    #[cfg(debug_assertions)]
    let app = app.layer(tower_livereload::LiveReloadLayer::new());

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3020);
    let listener = site::bind_with_fallback(port).await;
    let local = listener.local_addr().expect("listener has a local addr");
    tracing::info!("Listening on http://{local}");

    axum::serve(listener, app).await.unwrap();
}
