// @generated nextrs Vercel deployment adapter.
//
// DO NOT PUT APPLICATION LOGIC HERE. Vercel currently requires a Rust function
// at api/index.rs, so this file adapts the shared app from src/app.rs to
// vercel_runtime. If this project no longer deploys to Vercel, delete this file
// together with its `index` Cargo target, Vercel-only dependencies, and Vercel
// configuration.
//
// `StreamingVercelLayer` is a drop-in replacement for the upstream
// `vercel_runtime::axum::VercelLayer` that doesn't buffer text/html
// responses — see ../../crates/nextrs/src/vercel.rs for why.

use nextrs::vercel::StreamingVercelLayer;
use tower::ServiceBuilder;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    init_tracing();

    let app = ServiceBuilder::new()
        .layer(StreamingVercelLayer::new())
        .service(site::app());

    vercel_runtime::run(app).await
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(std::io::stdout)
        .json()
        .init();
}
