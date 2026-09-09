include!(concat!(env!("OUT_DIR"), "/nextrs_routes.rs"));
pub fn app() -> axum::Router {
    nextrs::router::build_router(generated_registry())
}
