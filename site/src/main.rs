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

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let listener = bind_with_fallback(port).await;
    let local = listener.local_addr().expect("listener has a local addr");
    tracing::info!("Listening on http://{local}");

    axum::serve(listener, app).await.unwrap();
}

/// Bind `0.0.0.0:start`, or the next free port up to `start + 20` if it's taken.
/// Beats panicking with a raw `AddrInUse` when something already holds the port.
async fn bind_with_fallback(start: u16) -> tokio::net::TcpListener {
    for port in start..start.saturating_add(20) {
        match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
            Ok(listener) => {
                if port != start {
                    tracing::warn!(
                        "Port {start} is in use; bound {port} instead (set PORT to choose)."
                    );
                }
                return listener;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(e) => {
                eprintln!("Failed to bind 0.0.0.0:{port}: {e}");
                std::process::exit(1);
            }
        }
    }
    eprintln!(
        "No free port in {start}..{}. Stop the process using it, or set PORT.",
        start.saturating_add(20)
    );
    std::process::exit(1);
}
