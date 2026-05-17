fn main() {
    nextrs::build::emit_registry("site/app", "api/index.rs", "nextrs_routes.rs")
        .expect("nextrs::build::emit_registry failed");

    // Mirror site/public/ into the workspace-root public/ so Vercel's CDN
    // picks it up. Source of truth lives next to site/app/; this dir is
    // gitignored.
    nextrs::build::sync_public_dir("site/public", "public")
        .expect("nextrs::build::sync_public_dir failed");
}
