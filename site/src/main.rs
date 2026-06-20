// The route registry is generated at build time by nextrs-build (see
// build.rs). Everything inside `nextrs_routes.rs` — the #[path] mod
// declarations and the `generated_registry()` function — is derived from a
// scan of the `app/` directory. Add a file under `app/`, save, and the next
// build picks it up.
use tracing_subscriber::EnvFilter;

include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

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

    // public/ sits next to app/. On Vercel the CDN serves it directly; locally
    // the framework wires ServeDir as a router fallback for the same URLs.
    // The compile-time path only resolves on the build machine, so deployments
    // that move the binary (e.g. Docker) point NEXTRS_PUBLIC_DIR at the
    // shipped copy of public/.
    let public_dir = std::env::var("NEXTRS_PUBLIC_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/public").to_string());
    let app = nextrs::router::build_router_with_public(generated_registry(), &public_dir)
        // Serve the OpenAPI document at /openapi.json. It's built from the
        // #[utoipa::path]-annotated route.rs handlers and drives the generated
        // TypeScript / React Query client (see site/client/).
        .merge(nextrs::openapi::spec_router(generated_openapi()));

    #[cfg(debug_assertions)]
    let app = app.layer(tower_livereload::LiveReloadLayer::new());

    let addr = format!(
        "0.0.0.0:{}",
        std::env::var("PORT").unwrap_or_else(|_| "3000".into())
    );
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
