//! Framework-owned OpenAPI extraction helper.
//!
//! `cargo nextrs client generate` invokes this binary. Application code belongs
//! in `src/app.rs` and `app/`; developers should not need to edit this file.

fn main() {
    let spec = react_todos::generated_openapi();
    let json = spec.to_pretty_json().expect("serialize OpenAPI document");
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/.nextrs/openapi.json");
    std::fs::write(out, json).expect("write .nextrs/openapi.json");
    eprintln!("wrote {out}");
}
