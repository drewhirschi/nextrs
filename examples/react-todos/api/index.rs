// @generated nextrs Vercel deployment adapter.
//
// DO NOT PUT APPLICATION LOGIC HERE. Vercel currently requires a Rust function
// at api/index.rs, so this file adapts the shared app from src/app.rs to
// vercel_runtime. If this project no longer deploys to Vercel, delete this file
// together with its `index` Cargo target, Vercel-only dependencies, and Vercel
// configuration.
//
// On Vercel: set this project's Root Directory to examples/react-todos and
// enable "Include files outside the Root Directory" (the crate path-depends on
// ../../nextrs). Static assets (page.tsx bundles + style.css) are served from
// public/ by the CDN; the catch-all rewrite (vercel.json) sends everything
// else here.

use nextrs::vercel::StreamingVercelLayer;
use tower::ServiceBuilder;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(std::io::stdout)
        .json()
        .init();

    let router = react_todos::with_cold_start_headers(react_todos::app());
    let app = ServiceBuilder::new()
        .layer(StreamingVercelLayer::new())
        .service(router);

    vercel_runtime::run(app).await
}
