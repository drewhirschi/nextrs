fn main() {
    nextrs::build::emit_registry("site/app", "api/index.rs", "nextrs_routes.rs")
        .expect("nextrs::build::emit_registry failed");

    // Docs: pre-render site/content/docs/*.md into includable Rust, plus
    // llms.txt and llms-full.txt (served by site/app/llms.txt/route.rs and
    // friends), all into OUT_DIR from the same sources.
    nextrs::docs::emit_docs(&nextrs::docs::DocsConfig {
        content_dir: "site/content/docs",
        base_url: "https://nextrs-umber.vercel.app",
        route_prefix: "/docs",
        site_name: "nextrs",
        site_description: "A Next.js-style routing framework for Rust built on Axum and Askama.",
    })
    .expect("nextrs::docs::emit_docs failed");

    // Mirror site/public/ into the workspace-root public/ so Vercel's CDN
    // picks it up. Source of truth lives next to site/app/; this dir is
    // gitignored.
    nextrs::build::sync_public_dir("site/public", "public")
        .expect("nextrs::build::sync_public_dir failed");
}
