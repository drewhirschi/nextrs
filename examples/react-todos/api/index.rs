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
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
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

    // Cold-start instrumentation: Vercel exposes no cold/warm signal, so the
    // function reports it. BOOT is captured once per instance; the first
    // request on a fresh (cold) instance flips FIRST_SEEN. The response carries
    // `x-cold: 1|0` and `x-init-ms` (process start → this request) so the
    // benchmark can bucket cold vs warm. See benchmarks/scripts/bench-cold.sh.
    static FIRST_SEEN: AtomicBool = AtomicBool::new(false);
    let boot = Instant::now();

    let router = nextrs::router::build_router(generated_registry())
        .merge(nextrs::openapi::spec_router(generated_openapi()))
        .layer(axum::middleware::map_response(move |mut res: axum::response::Response| async move {
            let cold = !FIRST_SEEN.swap(true, Ordering::Relaxed);
            let headers = res.headers_mut();
            headers.insert("x-cold", if cold { "1" } else { "0" }.parse().unwrap());
            if let Ok(v) = boot.elapsed().as_millis().to_string().parse() {
                headers.insert("x-init-ms", v);
            }
            res
        }));
    let app = ServiceBuilder::new()
        .layer(StreamingVercelLayer::new())
        .service(router);

    vercel_runtime::run(app).await
}
