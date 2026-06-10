fn main() {
    // Wire the app/ tree into a router and OpenAPI doc.
    nextrs::build::emit_registry("app", "src/main.rs", "nextrs_routes.rs")
        .expect("emit_registry failed");
    // Typed seed companions for props.rs (the React Query cache warming).
    nextrs::build::emit_seeds("app", "nextrs_seeds.rs").expect("emit_seeds failed");
    // Bundle the page.tsx entries into public/dist/ with rolldown.
    nextrs::bundle::bundle_pages(&nextrs::bundle::BundleConfig {
        app_dir: "app",
        client_dir: "client",
        client_alias: "@react-todos/client",
        public_dist: "public/dist",
    })
    .expect("bundle_pages failed");
}
