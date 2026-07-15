fn main() {
    nextrs::build::emit_registry("app", "src/main.rs", "nextrs_routes.rs")
        .expect("nextrs::build::emit_registry failed");

    // Docs: pre-render content/docs/*.md into includable Rust, plus llms.txt
    // and llms-full.txt (served by app/llms.txt/route.rs and friends), all
    // into OUT_DIR from the same sources.
    nextrs::docs::emit_docs(&nextrs::docs::DocsConfig {
        content_dir: "content/docs",
        base_url: "https://nextrs-docs.vercel.app",
        route_prefix: "/docs",
        site_name: "nextrs",
        site_description: "A Next.js-style routing framework for Rust built on Axum and Askama.",
    })
    .expect("nextrs::docs::emit_docs failed");

    // Bundle React convention files (page.tsx) into public/dist/ with rolldown.
    // The docs site is a hybrid: the landing is React (app/page.tsx), the docs
    // pages stay server-rendered. No-op when there are no .tsx pages.
    let assets = nextrs::bundle::bundle_pages(&nextrs::bundle::BundleConfig {
        app_dir: "app",
        client_dir: "client",
        client_alias: "@site/client",
        public_dist: "public/dist",
        ..Default::default()
    })
    .expect("nextrs::bundle::bundle_pages failed");
    println!(
        "cargo:rustc-env=NEXTRS_STYLE_URL={}",
        assets.stylesheet.as_deref().unwrap_or("/style.css")
    );
}
