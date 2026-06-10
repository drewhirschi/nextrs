fn main() {
    nextrs::build::emit_registry("app", "src/main.rs", "nextrs_routes.rs")
        .expect("nextrs::build::emit_registry failed");

    // Docs: pre-render content/docs/*.md into includable Rust, plus llms.txt
    // and llms-full.txt (served by app/llms.txt/route.rs and friends), all
    // into OUT_DIR from the same sources.
    nextrs::docs::emit_docs(&nextrs::docs::DocsConfig {
        content_dir: "content/docs",
        base_url: "https://nextrs-umber.vercel.app",
        route_prefix: "/docs",
        site_name: "nextrs",
        site_description: "A Next.js-style routing framework for Rust built on Axum and Askama.",
    })
    .expect("nextrs::docs::emit_docs failed");
}
