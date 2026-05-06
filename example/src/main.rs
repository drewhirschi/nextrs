use nextrs::conventions::{RouteEntry, RouteRegistry};
use nextrs::router::build_router;
use tower_http::services::ServeDir;
use tracing_subscriber::EnvFilter;

// Until codegen exists, we wire the app/ convention files into the binary by
// hand. Each `#[path]` mod declaration pulls in one convention file.
#[path = "../app/layout.rs"]
mod root_layout;
#[path = "../app/page.rs"]
mod root_page;

#[path = "../app/simple/page.rs"]
mod simple_page;

#[path = "../app/with-loading/page.rs"]
mod with_loading_page;
#[path = "../app/with-loading/loading.rs"]
mod with_loading_loading;

#[path = "../app/with-layout/layout.rs"]
mod with_layout_layout;
#[path = "../app/with-layout/page.rs"]
mod with_layout_page;
#[path = "../app/with-layout/loading.rs"]
mod with_layout_loading;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let mut registry = RouteRegistry::new();

    // Root: overview page wrapped in the root layout.
    registry.add(RouteEntry {
        path: "/".to_string(),
        page: Some(Box::new(|req| Box::pin(root_page::render(req)))),
        layout: Some(Box::new(root_layout::render)),
        loading: None,
        methods: vec![],
    });

    // 1. Just a page.
    registry.add(RouteEntry {
        path: "/simple".to_string(),
        page: Some(Box::new(|req| Box::pin(simple_page::render(req)))),
        layout: None,
        loading: None,
        methods: vec![],
    });

    // 2. Page + loading: streams the loading shell while the page resolves.
    registry.add(RouteEntry {
        path: "/with-loading".to_string(),
        page: Some(Box::new(|req| Box::pin(with_loading_page::render(req)))),
        layout: None,
        loading: Some(Box::new(with_loading_loading::render)),
        methods: vec![],
    });

    // 3. Layout + page + loading: nested layout (sidebar) wraps the page,
    //    loading shell renders inside the sidebar while the page resolves.
    registry.add(RouteEntry {
        path: "/with-layout".to_string(),
        page: Some(Box::new(|req| Box::pin(with_layout_page::render(req)))),
        layout: Some(Box::new(with_layout_layout::render)),
        loading: Some(Box::new(with_layout_loading::render)),
        methods: vec![],
    });

    // Static files in `public/` are served at root paths. On Vercel these are
    // delivered from the CDN (never reach the function); locally we serve them
    // via tower-http's ServeDir as a fallback. Routes win over static files
    // when both match the same path — opposite of Vercel's order, but the only
    // way that conflicts in practice is if you name a route the same as a
    // file, which you almost never do.
    let app = build_router(registry).fallback_service(ServeDir::new("public"));

    #[cfg(debug_assertions)]
    let app = app.layer(tower_livereload::LiveReloadLayer::new());

    let addr = "0.0.0.0:3000";
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
