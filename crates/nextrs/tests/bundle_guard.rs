//! Regression test for the docs-site dead-landing outage
//! (docs/postmortems/2026-07-11-docs-site-dead-landing.md).
//!
//! Rolldown only WARNS when a bare specifier can't be resolved, externalizing
//! it into the emitted module — a guaranteed TypeError in the browser.
//! `bundle_pages` must promote that warning to a build failure.
#![cfg(all(feature = "build", feature = "tsx"))]

use std::path::Path;

#[test]
fn unresolved_bare_import_fails_the_build() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/unresolved-bare");
    let out_dir = tempfile::tempdir().expect("tempdir");

    // The empty node_modules can't be committed (node_modules/ is gitignored,
    // and git doesn't track empty dirs anyway) — bundle_pages checks for its
    // existence before bundling, so create it here.
    std::fs::create_dir_all(fixture.join("client/node_modules")).expect("create node_modules");

    // bundle_pages reads build-script env vars; integration tests run
    // single-threaded per binary here, and this is the only test touching env.
    unsafe {
        std::env::set_var("CARGO_MANIFEST_DIR", &fixture);
        std::env::set_var("OUT_DIR", out_dir.path());
        std::env::remove_var("NEXTRS_SKIP_BUNDLE");
    }

    let err = nextrs::bundle::bundle_pages(&nextrs::bundle::BundleConfig {
        app_dir: "app",
        client_dir: "client",
        client_alias: "@fixture/client",
        public_dist: "public/dist",
        ..Default::default()
    })
    .expect_err("bundling a page with a missing bare dependency must fail");

    let msg = err.to_string();
    assert!(
        msg.contains("unresolved bare imports"),
        "error should name the failure class, got: {msg}"
    );
    assert!(
        msg.contains("package.json"),
        "error should point at the fix (add the dep to package.json), got: {msg}"
    );
}
