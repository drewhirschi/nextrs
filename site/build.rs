fn main() {
    nextrs::build::emit_registry("app", "src/main.rs", "nextrs_routes.rs")
        .expect("nextrs::build::emit_registry failed");
}
