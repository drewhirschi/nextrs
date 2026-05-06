// Vercel deployment entry point. Mirrors example/src/main.rs but hands the
// router to vercel_runtime::run via nextrs::vercel::StreamingVercelLayer
// instead of axum::serve. Codegen will generate this file once Phase 1 lands.
//
// `StreamingVercelLayer` is a drop-in replacement for the upstream
// `vercel_runtime::axum::VercelLayer` that doesn't buffer text/html responses.
// See nextrs/src/vercel.rs for the full story.

use nextrs::conventions::{RouteEntry, RouteRegistry};
use nextrs::router::build_router;
use nextrs::vercel::StreamingVercelLayer;
use tower::ServiceBuilder;

#[path = "../example/app/layout.rs"]
mod root_layout;
#[path = "../example/app/page.rs"]
mod root_page;

#[path = "../example/app/simple/page.rs"]
mod simple_page;

#[path = "../example/app/with-loading/page.rs"]
mod with_loading_page;
#[path = "../example/app/with-loading/loading.rs"]
mod with_loading_loading;

#[path = "../example/app/with-layout/layout.rs"]
mod with_layout_layout;
#[path = "../example/app/with-layout/page.rs"]
mod with_layout_page;
#[path = "../example/app/with-layout/loading.rs"]
mod with_layout_loading;

#[tokio::main]
async fn main() -> Result<(), vercel_runtime::Error> {
    let mut registry = RouteRegistry::new();

    registry.add(RouteEntry {
        path: "/".to_string(),
        page: Some(Box::new(|req| Box::pin(root_page::render(req)))),
        layout: Some(Box::new(root_layout::render)),
        loading: None,
        methods: vec![],
    });

    registry.add(RouteEntry {
        path: "/simple".to_string(),
        page: Some(Box::new(|req| Box::pin(simple_page::render(req)))),
        layout: None,
        loading: None,
        methods: vec![],
    });

    registry.add(RouteEntry {
        path: "/with-loading".to_string(),
        page: Some(Box::new(|req| Box::pin(with_loading_page::render(req)))),
        layout: None,
        loading: Some(Box::new(with_loading_loading::render)),
        methods: vec![],
    });

    registry.add(RouteEntry {
        path: "/with-layout".to_string(),
        page: Some(Box::new(|req| Box::pin(with_layout_page::render(req)))),
        layout: Some(Box::new(with_layout_layout::render)),
        loading: Some(Box::new(with_layout_loading::render)),
        methods: vec![],
    });

    let router = build_router(registry);
    let app = ServiceBuilder::new()
        .layer(StreamingVercelLayer::new())
        .service(router);

    vercel_runtime::run(app).await
}
