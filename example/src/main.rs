// The route registry is generated at build time by nextrs-build (see
// build.rs). Everything inside `nextrs_routes.rs` — the #[path] mod
// declarations and the `generated_registry()` function — is derived from a
// scan of the `app/` directory. Add a file under `app/`, save, and the next
// build picks it up.
use tower_http::services::ServeDir;
use tracing_subscriber::EnvFilter;

include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // Static files in `public/` are served at root paths. On Vercel these go
    // straight to the CDN; locally we serve them via tower-http's ServeDir as
    // a fallback for paths the router doesn't match.
    let app = nextrs::router::build_router(generated_registry())
        .fallback_service(ServeDir::new("public"));

    #[cfg(debug_assertions)]
    let app = app.layer(tower_livereload::LiveReloadLayer::new());

    let addr = "0.0.0.0:3000";
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
