// Vercel deployment entry point. The route registry is generated at build
// time by nextrs::build (see ../build.rs) from a scan of app/. Add a file
// under site/app/, save, redeploy. This bin (`index`) and the dev bin (`site`,
// src/main.rs) both `include!` the same generated registry.
//
// `StreamingVercelLayer` is a drop-in replacement for the upstream
// `vercel_runtime::axum::VercelLayer` that doesn't buffer text/html
// responses — see ../../crates/nextrs/src/vercel.rs for why.

use nextrs::vercel::StreamingVercelLayer;
use tower::ServiceBuilder;
use tracing_subscriber::EnvFilter;

include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    init_tracing();

    let router = nextrs::router::build_router(generated_registry())
        .merge(nextrs::openapi::spec_router(generated_openapi()));
    let app = ServiceBuilder::new()
        .layer(StreamingVercelLayer::new())
        .service(router);

    vercel_runtime::run(app).await
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(std::io::stdout)
        .json()
        .init();
}
