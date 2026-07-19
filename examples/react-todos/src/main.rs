// The route registry + OpenAPI doc are generated from the app/ tree at build
// time (see build.rs). Add a convention file under app/, rebuild, done.
use tracing_subscriber::EnvFilter;

include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // page.tsx bundles + style.css live in public/. NEXTRS_PUBLIC_DIR lets a
    // moved binary (Docker) point at the shipped copy.
    let public_dir = std::env::var("NEXTRS_PUBLIC_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/public").to_string());

    // App state as an Extension layer — the seedable-and-stateful paved road:
    // handlers extract it, and seed companions read it back out of the
    // request extensions during prefetch.
    let app = nextrs::router::build_router_with_public(generated_registry(), &public_dir)
        .merge(nextrs::openapi::spec_router(generated_openapi()))
        .layer(axum::Extension(react_todos::core::todos::TodosCtx::new()));

    let addr = format!(
        "0.0.0.0:{}",
        std::env::var("PORT").unwrap_or_else(|_| "3000".to_string())
    );
    tracing::info!("react-todos listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
