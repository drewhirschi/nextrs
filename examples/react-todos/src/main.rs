//! Local/container process entry point.
//!
//! The application itself is constructed in `src/app.rs`; keep this file
//! limited to local process setup and serving.

use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        // Saves each request's lines (and each job attempt's) for /__nx/admin.
        .with(nextrs::logs::layer())
        .init();

    let app = react_todos::app();

    let addr = format!(
        "0.0.0.0:{}",
        std::env::var("PORT").unwrap_or_else(|_| "3000".to_string())
    );
    tracing::info!("react-todos listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    // Jobs self-deliver over HTTP; tell them where this server actually bound.
    nextrs::jobs::announce_local_addr(listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}
