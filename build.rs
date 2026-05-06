fn main() {
    nextrs_build::emit_registry("example/app", "api/index.rs", "nextrs_routes.rs")
        .expect("nextrs_build::emit_registry failed");
}
