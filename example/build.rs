fn main() {
    nextrs_build::emit_registry("app", "src/main.rs", "nextrs_routes.rs")
        .expect("nextrs_build::emit_registry failed");
}
