// Vercel deployment entry. Same generated registry + OpenAPI doc the local
// server (src/main.rs) uses, wrapped for vercel_runtime. StreamingVercelLayer
// is the html-streaming-safe replacement for the upstream VercelLayer.
//
// On Vercel: set this project's Root Directory to examples/react-todos and
// enable "Include files outside the Root Directory" (the crate path-depends on
// ../../nextrs). Static assets (page.tsx bundles + style.css) are served from
// public/ by the CDN; the catch-all rewrite (vercel.json) sends everything
// else here.

use nextrs::vercel::StreamingVercelLayer;
use tower::ServiceBuilder;
use tracing_subscriber::EnvFilter;

include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(std::io::stdout)
        .json()
        .init();

    let router = nextrs::router::build_router(generated_registry())
        .merge(nextrs::openapi::spec_router(generated_openapi()));
    let app = ServiceBuilder::new()
        .layer(StreamingVercelLayer::new())
        .service(router);

    vercel_runtime::run(app).await
}
