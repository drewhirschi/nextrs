fn main() {
    nextrs::build::emit_registry("site/app", "api/index.rs", "nextrs_routes.rs")
        .expect("nextrs::build::emit_registry failed");
}
